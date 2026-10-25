//! The searches Sourcegraph plans for a query sent with `patternType:
//! keyword`, each with its filters. `plan/examples.jsonl` holds the plans a
//! Sourcegraph server gives a corpus of queries, and this must match each.

use std::collections::{VecDeque, vec_deque};
use std::mem::take;

use super::lint::KNOWN;

/// The most searches the rules that read each search's filters take; a plan
/// of 64 or more is unread to them.
const MAX_LISTED: usize = 63;
const TYPES: [(&str, PatternType); 8] = [
    ("keyword", PatternType::Keyword),
    ("standard", PatternType::Standard),
    ("lucky", PatternType::Standard),
    ("regexp", PatternType::Regexp),
    ("regex", PatternType::Regexp),
    ("literal", PatternType::Literal),
    ("structural", PatternType::Structural),
    ("codycontext", PatternType::CodyContext),
];
const PREDICATES: [(&str, &str); 3] = [
    (
        "repo r",
        "contains.file has.file contains.path has.path contains.content \
         has.content contains.commit.after has.commit.after has.description \
         has.meta has.topic has.tag has has.key contains",
    ),
    ("file f path", "contains.content has.content has.owner has.contributor"),
    ("rev revision", "at.time"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternType {
    Keyword,
    Standard,
    Regexp,
    Literal,
    Structural,
    CodyContext,
}

impl PatternType {
    pub fn named(value: &str) -> Option<Self> {
        TYPES.iter().find(|t| t.0 == value).map(|t| t.1)
    }

    pub fn name(self) -> &'static str {
        TYPES.iter().find(|t| t.1 == self).map_or("", |t| t.0)
    }
}

/// A filter of a planned search: the field lower-cased with its aliases
/// resolved, and the value as the server reads it.
#[derive(Clone, Debug, Eq)]
pub struct Filter {
    pub field: String,
    pub value: String,
    pub negated: bool,
    pub predicate: bool,
    at: usize,
}

impl PartialEq for Filter {
    fn eq(&self, other: &Self) -> bool {
        (&self.field, &self.value, self.negated, self.predicate)
            == (&other.field, &other.value, other.negated, other.predicate)
    }
}

pub enum Plan {
    Read(Searches),
    Rejected,
    /// A query of 4 Gi characters or more, past the narrower indices that
    /// keep a reading small.
    Unread,
}

pub struct Searches {
    pub pattern_type: PatternType,
    root: Option<Node>,
}

enum Node {
    Filter(Box<Filter>),
    Pattern(Pattern),
    Op(Box<Op>),
}

struct Pattern {
    at: usize,
    negated: bool,
    empty: bool,
    slash: bool,
}

/// An operator, with the filters and patterns inside it counted, so that
/// no question about a plan recurses through it.
struct Op {
    kind: Kind,
    run: Run,
}

impl Drop for Op {
    // A plan can nest hundreds of thousands of levels deep; dropping it
    // level by level would take a stack frame for each.
    fn drop(&mut self) {
        let mut items = take(&mut self.run.items);
        while let Some(node) = items.pop_back() {
            if let Node::Op(mut op) = node {
                items.append(&mut op.run.items);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    And,
    Or,
    Seq,
}

pub fn plan(query: &str) -> Plan {
    let mut ty = PatternType::Keyword;
    if query.chars().all(char::is_whitespace) {
        return Plan::Read(Searches { pattern_type: ty, root: None });
    }
    // Narrower indices keep the reading small; no command line comes near
    // 4 Gi characters.
    let Some(text) = Text::new(query) else { return Plan::Unread };
    let literal = read(&text, PatternType::Literal);
    if let Ok(reading) = &literal {
        for f in reading.root.filters() {
            if f.field.eq_ignore_ascii_case("patterntype") {
                let named = TYPES.iter().find(|t| t.0 == f.value);
                ty = named.map_or(ty, |t| t.1);
            }
        }
    }
    let reading = if ty == PatternType::Literal {
        literal
    } else {
        drop(literal);
        read(&text, ty)
    };
    drop(text);
    let Ok(reading) = reading else { return Plan::Rejected };
    let mut root = reading.root;
    if reading.flat || !reading.sharing_off {
        root = share_filters(root);
    }
    let root = resolve(root, ty);
    Plan::Read(Searches { pattern_type: ty, root: Some(root) })
}

impl Plan {
    /// Each search's filters, unless the plan isn't read or holds 64
    /// searches or more.
    pub fn each(&self) -> Option<Vec<Vec<&Filter>>> {
        match self {
            Plan::Read(searches) => searches.list(MAX_LISTED),
            _ => None,
        }
    }
}

impl Searches {
    /// How many searches the plan holds, saturating at `u64::MAX`.
    pub fn count(&self) -> u64 {
        self.root.as_ref().map_or(0, Node::count)
    }

    /// Each search's filters, in the server's order, when there are at most
    /// `limit` searches.
    pub fn list<'a>(&'a self, limit: usize) -> Option<Vec<Vec<&'a Filter>>> {
        if self.count() > u64::try_from(limit).unwrap_or(u64::MAX) {
            return None;
        }
        let step = |mut sofar: Vec<&'a Filter>, f: &'a Filter| {
            sofar.push(f);
            sofar
        };
        let root = self.root.as_ref();
        Some(
            root.map_or_else(Vec::new, |r| r.walk(vec![vec![]], &step, false)),
        )
    }

    /// The states that folding `step` over each search's filters, from
    /// `start`, ends in, for every search however many there are.
    pub fn reach<S: Clone + PartialEq>(
        &self,
        start: S,
        step: impl Fn(S, &Filter) -> S,
    ) -> Vec<S> {
        self.root
            .as_ref()
            .map_or_else(Vec::new, |root| root.walk(vec![start], &step, true))
    }
}

fn op(kind: Kind, items: impl IntoIterator<Item = Node>) -> Node {
    let mut run = Run::default();
    for item in items {
        run.push_back(item);
    }
    Node::Op(Box::new(Op { kind, run }))
}

fn pattern(at: usize, negated: bool, empty: bool, slash: bool) -> Node {
    Node::Pattern(Pattern { at, negated, empty, slash })
}

/// Items being combined into an operator, with the filters and patterns
/// inside them counted. Joining two runs moves the shorter, so building a
/// plan moves no item more than about log2 of the plan's size times.
#[derive(Default)]
struct Run {
    items: VecDeque<Node>,
    filters: usize,
    patterns: usize,
}

impl Run {
    fn add(&mut self, node: &Node) {
        let (filters, patterns) = node.leaves();
        self.filters += filters;
        self.patterns += patterns;
    }

    fn subtract(&mut self, node: &Node) {
        let (filters, patterns) = node.leaves();
        self.filters -= filters;
        self.patterns -= patterns;
    }

    fn push_back(&mut self, node: Node) {
        self.add(&node);
        self.items.push_back(node);
    }

    fn push_front(&mut self, node: Node) {
        self.add(&node);
        self.items.push_front(node);
    }

    fn pop_front(&mut self) -> Option<Node> {
        let node = self.items.pop_front()?;
        self.subtract(&node);
        Some(node)
    }

    /// Puts `later`'s items after this run's.
    fn append(&mut self, mut later: Run) {
        if later.items.len() > self.items.len() {
            std::mem::swap(self, &mut later);
            while let Some(node) = later.items.pop_back() {
                self.push_front(node);
            }
        } else {
            while let Some(node) = later.items.pop_front() {
                self.push_back(node);
            }
        }
    }

    /// Adds an item after the first: an empty one goes, and one of `kind`
    /// gives its items, at every depth. Within an operator of `kind`, only
    /// the first item can be empty or of `kind` too, so the run taken in is
    /// looked at only from the front.
    fn take_in(&mut self, kind: Kind, node: Node) {
        let mut inner = Run::default();
        inner.push_back(node);
        while let Some(front) = inner.pop_front() {
            match front {
                Node::Filter(f) if f.value.is_empty() => {}
                Node::Pattern(p) if p.empty => {}
                Node::Op(mut op) if op.kind == kind => {
                    let mut outer = take(&mut op.run);
                    outer.append(inner);
                    inner = outer;
                }
                front => {
                    inner.push_front(front);
                    break;
                }
            }
        }
        self.append(inner);
    }
}

/// Combines items as the server does: empty filters go unless nothing else
/// is left, empty patterns go unless first, and an item of the same kind is
/// replaced by its items, at every depth, except a first one followed by
/// another kind of operator, which keeps its own.
fn combine(kind: Kind, items: impl IntoIterator<Item = Node>) -> Node {
    let mut items = items.into_iter();
    let mut empty = None;
    let first = loop {
        match items.next() {
            Some(Node::Filter(f)) if f.value.is_empty() => {
                empty = Some(Node::Filter(f));
            }
            Some(first) => break first,
            None => return empty.unwrap_or_else(|| op(kind, [])),
        }
    };
    let mut rest = Run::default();
    for item in items {
        rest.take_in(kind, item);
    }
    let nested = matches!(
        rest.items.front(),
        Some(Node::Op(second)) if second.kind != kind
    );
    let mut all = match first {
        Node::Op(mut op) if op.kind == kind && !nested => take(&mut op.run),
        first => {
            let mut run = Run::default();
            run.push_back(first);
            run
        }
    };
    all.append(rest);
    if all.items.len() == 1 {
        return all.items.pop_front().unwrap_or_else(|| unreachable!());
    }
    Node::Op(Box::new(Op { kind, run: all }))
}

impl Node {
    /// The filters and the patterns inside.
    fn leaves(&self) -> (usize, usize) {
        match self {
            Node::Filter(_) => (1, 0),
            Node::Pattern(_) => (0, 1),
            Node::Op(op) => (op.run.filters, op.run.patterns),
        }
    }

    fn has_filter(&self) -> bool {
        self.leaves().0 > 0
    }

    /// The searches, saturating at `u64::MAX`: an `or` with a filter
    /// inside sums its items', any other operator with one multiplies them.
    fn count(&self) -> u64 {
        let mut counting: Vec<(&Op, usize, u64)> = Vec::new();
        let mut node = self;
        loop {
            let mut value = match node {
                Node::Op(op) if op.run.filters > 0 => {
                    counting.push((op, 0, u64::from(op.kind != Kind::Or)));
                    None
                }
                _ => Some(1),
            };
            loop {
                let Some((op, next, total)) = counting.last_mut() else {
                    return value.unwrap_or(1);
                };
                if let Some(n) = value.take() {
                    *total = if op.kind == Kind::Or {
                        total.saturating_add(n)
                    } else {
                        total.saturating_mul(n)
                    };
                }
                if let Some(item) = op.run.items.get(*next) {
                    *next += 1;
                    node = item;
                    break;
                }
                value = counting.pop().map(|(_, _, total)| total);
            }
        }
    }

    /// The first and last offsets of the patterns inside.
    fn span(&self) -> (usize, usize) {
        let mut span = (usize::MAX, 0);
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            match node {
                Node::Pattern(p) => {
                    span = (span.0.min(p.at), span.1.max(p.at));
                }
                Node::Op(op) => pending.extend(op.run.items.iter()),
                Node::Filter(_) => {}
            }
        }
        span
    }

    /// Every filter inside, in the order written into the tree.
    fn filters(&self) -> Vec<&Filter> {
        let mut out = Vec::new();
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            match node {
                Node::Filter(f) => out.push(f.as_ref()),
                Node::Pattern(_) => {}
                Node::Op(op) => pending.extend(op.run.items.iter().rev()),
            }
        }
        out
    }

    /// The states folding `step` over each search's filters ends in, in
    /// the order of the searches, each once when `once`.
    fn walk<'a, S: Clone + PartialEq>(
        &'a self,
        states: Vec<S>,
        step: &impl Fn(S, &'a Filter) -> S,
        once: bool,
    ) -> Vec<S> {
        /// An operator being walked: its next item, the states it threads
        /// through its items (for an `or`, those each item starts from),
        /// and for an `or` the ends so far.
        struct Walking<'a, S> {
            op: &'a Op,
            next: usize,
            states: Vec<S>,
            ends: Vec<S>,
        }
        let mut walking: Vec<Walking<'a, S>> = Vec::new();
        let (mut node, mut states) = (self, states);
        loop {
            let mut ends = match node {
                Node::Filter(f) => Some(
                    states.drain(..).map(|s| step(s, f.as_ref())).collect(),
                ),
                Node::Op(op) if op.run.filters > 0 => {
                    let (op, next) = (op.as_ref(), 0);
                    let (states, ends) = (take(&mut states), vec![]);
                    walking.push(Walking { op, next, states, ends });
                    None
                }
                _ => Some(take(&mut states)),
            };
            loop {
                if let Some(found) = ends.take() {
                    let found: Vec<S> =
                        if once { distinct(found) } else { found };
                    let Some(top) = walking.last_mut() else { return found };
                    if top.op.kind == Kind::Or {
                        top.ends.extend(found);
                    } else {
                        top.states = found;
                    }
                }
                let Some(top) = walking.last_mut() else { unreachable!() };
                let op = top.op;
                if let Some(item) = op.run.items.get(top.next) {
                    top.next += 1;
                    node = item;
                    states = if op.kind == Kind::Or {
                        top.states.clone()
                    } else {
                        take(&mut top.states)
                    };
                    break;
                }
                let Some(done) = walking.pop() else { unreachable!() };
                ends = Some(if op.kind == Kind::Or {
                    done.ends
                } else {
                    done.states
                });
            }
        }
    }
}

fn distinct<S: PartialEq>(all: Vec<S>) -> Vec<S> {
    let mut out: Vec<S> = Vec::new();
    for s in all {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

/// Rebuilds a tree from its leaves up, without recursing: `leaf` maps each
/// leaf, and `build` makes each operator from its rebuilt items.
fn rebuild(
    root: Node,
    leaf: &impl Fn(Node) -> Node,
    build: &impl Fn(Kind, Vec<Node>) -> Node,
) -> Node {
    let mut frames: Vec<(Kind, vec_deque::IntoIter<Node>, Vec<Node>)> =
        Vec::new();
    let mut node = root;
    loop {
        let mut value = match node {
            Node::Op(mut op) => {
                frames.push((
                    op.kind,
                    take(&mut op.run.items).into_iter(),
                    Vec::new(),
                ));
                None
            }
            node => Some(leaf(node)),
        };
        loop {
            let Some((_, items, done)) = frames.last_mut() else {
                return value.unwrap_or_else(|| unreachable!());
            };
            done.extend(value.take());
            if let Some(next) = items.next() {
                node = next;
                break;
            }
            let Some((kind, _, done)) = frames.pop() else { unreachable!() };
            value = Some(build(kind, done));
        }
    }
}

/// The shared-filter rule: an `or` whose first operand starts with filters
/// and whose last ends with them shares those filters with every operand,
/// behind any filters that lead the query.
fn share_filters(root: Node) -> Node {
    let Node::Op(mut top) = root else { return root };
    let kind = top.kind;
    let (mut front, mut operands) = match kind {
        Kind::Or => (VecDeque::new(), take(&mut top.run.items)),
        Kind::And => {
            let mut lead = take(&mut top.run.items);
            let filters = |lead: &VecDeque<Node>| {
                lead.iter().all(|n| matches!(n, Node::Filter(_)))
            };
            match lead.pop_back() {
                Some(Node::Op(mut or))
                    if or.kind == Kind::Or && filters(&lead) =>
                {
                    (lead, take(&mut or.run.items))
                }
                last => {
                    lead.extend(last);
                    return op(Kind::And, lead);
                }
            }
        }
        Kind::Seq => return Node::Op(top),
    };
    if !shared(&operands) {
        let or = op(Kind::Or, operands);
        if kind == Kind::Or {
            return or;
        }
        front.push_back(or);
        return op(Kind::And, front);
    }
    let (Some(first), Some(last)) =
        (operands.pop_front(), operands.pop_back())
    else {
        unreachable!()
    };
    let (first, last) = (parts(first, &mut front), parts(last, &mut front));
    operands.push_front(first);
    operands.push_back(last);
    front.push_back(combine(Kind::Or, operands));
    combine(Kind::And, front)
}

/// Moves an end operand's filters to `front`, leaving its pattern part.
fn parts(node: Node, front: &mut VecDeque<Node>) -> Node {
    let Node::Op(mut op) = node else { return node };
    if op.run.filters == 0 {
        return Node::Op(op);
    }
    let (filters, rest): (Vec<Node>, Vec<Node>) = take(&mut op.run.items)
        .into_iter()
        .partition(|n| matches!(n, Node::Filter(_)));
    front.extend(filters);
    combine(Kind::And, rest)
}

fn shared(operands: &VecDeque<Node>) -> bool {
    let split = |node: &Node| -> Option<(Vec<usize>, (usize, usize))> {
        if !node.has_filter() {
            return Some((Vec::new(), node.span()));
        }
        let Node::Op(op) = node else { return None };
        let mut filters = Vec::new();
        let mut span = (usize::MAX, 0);
        for item in &op.run.items {
            match item {
                Node::Filter(f) => filters.push(f.at),
                item if !item.has_filter() => {
                    let (lo, hi) = item.span();
                    span = (span.0.min(lo), span.1.max(hi));
                }
                _ => return None,
            }
        }
        (op.kind == Kind::And && span.0 != usize::MAX)
            .then_some((filters, span))
    };
    let (Some(first), Some(last)) = (operands.front(), operands.back()) else {
        return false;
    };
    let (Some((lead, (start, _))), Some((trail, (_, end)))) =
        (split(first), split(last))
    else {
        return false;
    };
    start != 0
        && lead.iter().all(|&at| at < start)
        && trail.iter().all(|&at| at > end)
        && operands.range(1..operands.len() - 1).all(|n| !n.has_filter())
}

fn renamed(mut f: Filter) -> Node {
    let field = f.field.to_ascii_lowercase();
    let field = match field.as_str() {
        "r" => "repo",
        "f" | "path" => "file",
        "l" | "language" => "lang",
        "since" => "after",
        "until" => "before",
        "m" | "msg" => "message",
        "revision" => "rev",
        "content" => {
            return pattern(f.at, f.negated, f.value.is_empty(), false);
        }
        other => other,
    };
    field.clone_into(&mut f.field);
    if field == "count" && f.value.eq_ignore_ascii_case("all") {
        "99999999".clone_into(&mut f.value);
    }
    Node::Filter(Box::new(f))
}

/// Field names are lower-cased with aliases resolved, `content:` becomes a
/// pattern and `count:all` a number. Each sequence of patterns becomes, in
/// keyword mode, an `and`; in the other modes its items go into the
/// expression holding it, with runs of plain patterns joined, and a first
/// item that isn't a pattern dropped.
///
/// A sequence is first built as a `Seq` of the items it spills, for the
/// operator holding it to take in.
fn resolve(root: Node, ty: PatternType) -> Node {
    let build = |kind, items: Vec<Node>| {
        let mut all = Vec::with_capacity(items.len());
        for item in items {
            match item {
                Node::Op(mut seq)
                    if seq.kind == Kind::Seq && kind == Kind::Seq =>
                {
                    all.push(combine(Kind::And, take(&mut seq.run.items)));
                }
                Node::Op(mut seq) if seq.kind == Kind::Seq => {
                    all.extend(take(&mut seq.run.items));
                }
                item => all.push(item),
            }
        }
        if kind == Kind::Seq {
            op(Kind::Seq, spill(all, ty))
        } else {
            combine(kind, all)
        }
    };
    let leaf = |node| match node {
        Node::Filter(f) => renamed(*f),
        node => node,
    };
    match rebuild(root, &leaf, &build) {
        Node::Op(mut seq) if seq.kind == Kind::Seq => {
            combine(Kind::And, take(&mut seq.run.items))
        }
        root => root,
    }
}

fn spill(mut resolved: Vec<Node>, ty: PatternType) -> Vec<Node> {
    if ty == PatternType::Keyword {
        return vec![combine(Kind::And, resolved)];
    }
    match &resolved[0] {
        Node::Pattern(p) if !p.negated => {}
        Node::Pattern(_) => return resolved,
        _ => {
            resolved.remove(0);
            return resolved;
        }
    }
    let apart = matches!(ty, PatternType::Standard | PatternType::CodyContext);
    let joins = |p: &Pattern| !(p.negated || apart && p.slash);
    let mut out: Vec<Node> = Vec::with_capacity(resolved.len());
    for item in resolved {
        if let (Some(Node::Pattern(last)), Node::Pattern(p)) =
            (out.last_mut(), &item)
            && joins(last)
            && joins(p)
        {
            last.empty &= p.empty;
        } else {
            out.push(item);
        }
    }
    out
}

fn ascii_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0B' | '\x0C' | '\r' | ' ')
}

fn known(letters: &[char]) -> bool {
    letters.len() <= 18 && {
        let field: String =
            letters.iter().map(char::to_ascii_lowercase).collect();
        KNOWN.contains(&field.as_str())
    }
}

fn starts(c: &[char], i: usize, word: &str) -> bool {
    word.chars()
        .enumerate()
        .all(|(k, w)| c.get(i + k).is_some_and(|c| c.eq_ignore_ascii_case(&w)))
}

/// The query's characters, with what a balanced run needs worked out once,
/// so that reading takes time linear in the query's length. A run starts after a character that isn't `\`, so its
/// escapes pair up as they do read from the start of the query.
struct Text {
    c: Vec<char>,
    /// The end of the run of ASCII letters from each index.
    letters: Vec<u32>,
    /// The parentheses open before each index, counted up from the fewest
    /// any index has, so that the counts index an array from 0.
    depth: Vec<u32>,
    stop: Vec<bool>,
    /// The first unescaped whitespace or `)` at or after each index whose
    /// depth equals the index's.
    level_end: Vec<u32>,
    /// The first unescaped `(` or whitespace at or after each index that a
    /// stop follows.
    stopped: Vec<u32>,
    /// For an unescaped `(`, the index of its `)`, or `NONE`.
    close: Vec<u32>,
    first: usize,
}

const NONE: u32 = u32::MAX;

/// An index into a `Text`, which holds fewer than `NONE` characters.
fn ix(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(NONE)
}

impl Text {
    #[expect(clippy::many_single_char_names, reason = "indices into `c`")]
    fn new(query: &str) -> Option<Text> {
        let c: Vec<char> = query.chars().collect();
        let n = c.len();
        if ix(n) == NONE {
            return None;
        }
        let mut escaped = vec![false; n];
        let mut i = 0;
        while i < n {
            if c[i] == '\\' && i + 1 < n {
                escaped[i + 1] = true;
                i += 1;
            }
            i += 1;
        }
        let change = |i: usize| -> i32 {
            match c[i] {
                '(' if !escaped[i] => 1,
                ')' if !escaped[i] => -1,
                _ => 0,
            }
        };
        let (mut d, mut low, mut high) = (0i64, 0i64, 0i64);
        for i in 0..n {
            d += i64::from(change(i));
            (low, high) = (low.min(d), high.max(d));
        }
        let mut depth = vec![u32::try_from(-low).ok()?; n + 1];
        for i in 0..n {
            depth[i + 1] = depth[i].checked_add_signed(change(i))?;
        }
        // The counts run from 0 to `high - low`, one entry each; a `(`
        // looks one level up, to a count some later index has.
        let levels = usize::try_from(high - low).ok()? + 1;
        let mut letters = vec![ix(n); n + 1];
        let mut stop = vec![false; n + 1];
        let mut level_end = vec![ix(n); n];
        let mut stopped = vec![ix(n); n + 1];
        let mut close = vec![NONE; n];
        let (mut near, mut near_close) =
            (vec![ix(n); levels], vec![NONE; levels]);
        for i in (0..n).rev() {
            letters[i] = if c[i].is_ascii_alphabetic() {
                letters[i + 1]
            } else {
                ix(i)
            };
            let j = i + usize::from(c[i] == '-');
            let end = letters[j] as usize;
            stop[i] = (end > j
                && c.get(end) == Some(&':')
                && known(&c[j..end]))
                || ["and ", "or ", "not "].iter().any(|w| starts(&c, i, w));
            let (d, plain) = (depth[i] as usize, !escaped[i]);
            if plain && (c[i].is_whitespace() || c[i] == ')') {
                near[d] = ix(i);
            }
            if plain && c[i] == ')' {
                near_close[d] = ix(i);
            }
            if plain && c[i] == '(' {
                close[i] = near_close[d + 1];
            }
            level_end[i] = near[d];
            let opens = plain && (c[i] == '(' || c[i].is_whitespace());
            stopped[i] =
                if opens && stop[i + 1] { ix(i) } else { stopped[i + 1] };
        }
        let first = c.iter().position(|c| !c.is_whitespace()).unwrap_or(n);
        Some(Text {
            c,
            letters,
            depth,
            stop,
            level_end,
            stopped,
            close,
            first,
        })
    }

    /// The end of the balanced run from `s`, or `None` when it fails.
    fn run(&self, s: usize) -> Option<usize> {
        let n = self.c.len();
        if s == n {
            return Some(n);
        }
        let end = self.level_end[s] as usize;
        let unbalanced = end == n && self.depth[n] != self.depth[s];
        let stopped = self.stopped[s] as usize;
        (!self.stop[s] && stopped >= end && !unbalanced).then_some(end)
    }

    /// The end of a plain run from `s`: to whitespace, or a parenthesis
    /// too with `parens`, a `\` keeping the next character with `escapes`.
    fn plain(&self, s: usize, parens: bool, escapes: bool) -> usize {
        let mut i = s;
        while let Some(&c) = self.c.get(i) {
            if escapes && c == '\\' {
                i += 2;
                continue;
            }
            if c.is_whitespace() || (parens && (c == '(' || c == ')')) {
                break;
            }
            i += 1;
        }
        i.min(self.c.len())
    }

    /// The index of the unescaped character that closes the quote or slash
    /// at `open`.
    fn closing(&self, open: usize) -> Option<usize> {
        let mut i = open + 1;
        while let Some(&c) = self.c.get(i) {
            if c == '\\' {
                i += 2;
            } else if c == self.c[open] {
                return Some(i);
            } else {
                i += 1;
            }
        }
        None
    }

    fn terminal(&self, i: usize) -> bool {
        self.c.get(i).is_none_or(|&c| ascii_space(c) || c == ')')
    }

    /// A quoted filter value's text, its escapes read; `None` for an escape
    /// the server doesn't know.
    fn unquote(&self, open: usize, close: usize) -> Option<String> {
        let mut out = String::new();
        let mut i = open + 1;
        while i < close {
            let c = self.c[i];
            i += 1;
            if c != '\\' {
                out.push(c);
                continue;
            }
            let e = self.c[i];
            i += 1;
            match e {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'a' | 'b' | 'f' | 'v' => out.extend(['\\', e]),
                _ if e == '\\' || e == self.c[open] => out.push(e),
                _ => return None,
            }
        }
        Some(out)
    }

    /// The end of a predicate value starting at `s`, for the field as
    /// written.
    fn predicate(&self, field: &str, s: usize) -> Option<usize> {
        let (_, names) = PREDICATES
            .iter()
            .find(|(fields, _)| fields.split(' ').any(|f| f == field))?;
        let open = (s..self.c.len()).find(|&i| {
            !(self.c[i].is_ascii_alphabetic() || self.c[i] == '.')
        })?;
        let name = self.text(s, open);
        let close = self.close[open];
        (close != NONE && names.split_whitespace().any(|n| n == name))
            .then(|| close as usize + 1)
    }

    fn text(&self, start: usize, end: usize) -> String {
        self.c[start..end].iter().collect()
    }
}

enum Halt {
    Reject,
    Missing,
}

struct Reading {
    root: Node,
    flat: bool,
    sharing_off: bool,
}

/// Reads the query in `ty` with groups, or, when a group is left open or
/// an operand is missing, with every parenthesis as text.
fn read(text: &Text, ty: PatternType) -> Result<Reading, Halt> {
    let mut groups = Reader::new(text, ty, false);
    match groups.read() {
        Ok(root) if groups.open == 0 => {
            let strict =
                matches!(ty, PatternType::Standard | PatternType::Literal);
            if strict && groups.mixed {
                return Err(Halt::Reject);
            }
            let sharing_off = groups.sharing_off;
            return Ok(Reading { root, flat: false, sharing_off });
        }
        Ok(_) | Err(Halt::Missing) => {}
        Err(halt) => return Err(halt),
    }
    match Reader::new(text, ty, true).read() {
        Ok(root) => Ok(Reading { root, flat: true, sharing_off: false }),
        Err(_) => Err(Halt::Reject),
    }
}

/// An or-expression being read: its operands, the levels of the operand
/// being read, and the items of the level being read.
#[derive(Default)]
struct Frame {
    operands: Vec<Node>,
    levels: Vec<Node>,
    items: Vec<Node>,
}

impl Frame {
    /// Ends the level being read, noting in `mixed` a sequence of patterns
    /// that holds a group read as `and` or `or`.
    fn end_level(&mut self, mixed: &mut bool) -> Result<(), Halt> {
        if self.items.is_empty() {
            return Err(Halt::Missing);
        }
        let (mut level, patterns): (Vec<Node>, Vec<Node>) =
            self.items.drain(..).partition(|n| n.leaves().1 == 0);
        if patterns.len() > 1 {
            let op =
                |n: &Node| matches!(n, Node::Op(op) if op.kind != Kind::Seq);
            *mixed |= patterns.iter().any(op);
            level.push(combine(Kind::Seq, patterns));
        } else {
            level.extend(patterns);
        }
        self.levels.push(combine(Kind::And, level));
        Ok(())
    }

    fn end_operand(&mut self) {
        let levels = take(&mut self.levels);
        self.operands.push(combine(Kind::And, levels));
    }

    fn end(mut self, mixed: &mut bool) -> Result<Node, Halt> {
        self.end_level(mixed)?;
        self.end_operand();
        Ok(combine(Kind::Or, self.operands))
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about one reading, not a state machine"
)]
struct Reader<'t> {
    t: &'t Text,
    ty: PatternType,
    flat: bool,
    pos: usize,
    open: usize,
    or_seen: bool,
    close_seen: bool,
    /// The shared-filter rule is off: a group opened after an `or`, or an
    /// `or` followed a `)`.
    sharing_off: bool,
    mixed: bool,
}

impl<'t> Reader<'t> {
    fn new(t: &'t Text, ty: PatternType, flat: bool) -> Self {
        Reader {
            t,
            ty,
            flat,
            pos: 0,
            open: 0,
            or_seen: false,
            close_seen: false,
            sharing_off: false,
            mixed: false,
        }
    }

    /// A `)` ends the innermost or-expression being read, unless `or`
    /// follows it: then that or-expression carries on, though its group is
    /// closed.
    fn read(&mut self) -> Result<Node, Halt> {
        let t = self.t;
        let mut frames = vec![Frame::default()];
        loop {
            while t.c.get(self.pos).is_some_and(|c| c.is_whitespace()) {
                self.pos += 1;
            }
            let Some(frame) = frames.last_mut() else { unreachable!() };
            let Some(&c) = t.c.get(self.pos) else {
                let mut node = None;
                while let Some(mut frame) = frames.pop() {
                    frame.items.extend(node);
                    node = Some(frame.end(&mut self.mixed)?);
                }
                return node.ok_or(Halt::Missing);
            };
            if !self.flat && c == '(' {
                if self.ty != PatternType::Keyword
                    && let Some(end) = t.run(self.pos)
                {
                    frame.items.push(pattern(self.pos, false, false, false));
                    self.pos = end;
                    continue;
                }
                self.sharing_off |= self.or_seen;
                self.open += 1;
                self.pos += 1;
                frames.push(Frame::default());
            } else if !self.flat && c == ')' {
                self.open = self.open.checked_sub(1).ok_or(Halt::Reject)?;
                self.close_seen = true;
                if frame.items.is_empty() {
                    frame.items.push(pattern(self.pos, false, false, false));
                }
                self.pos += 1;
                if starts(&t.c, self.pos, "or") {
                    self.pos += 2;
                    frame.end_level(&mut self.mixed)?;
                    frame.end_operand();
                } else if let Some(frame) = frames.pop() {
                    let group = frame.end(&mut self.mixed)?;
                    let Some(outer) = frames.last_mut() else {
                        unreachable!()
                    };
                    outer.items.push(group);
                }
            } else if let Some((or, len)) = self.operator() {
                frame.end_level(&mut self.mixed)?;
                if or {
                    self.sharing_off |= self.close_seen;
                    self.or_seen = true;
                    frame.end_operand();
                }
                self.pos += len;
            } else {
                let item = self.item()?;
                let Some(frame) = frames.last_mut() else { unreachable!() };
                frame.items.push(item);
            }
        }
    }

    fn operator(&self) -> Option<(bool, usize)> {
        let (c, s) = (&self.t.c, self.pos);
        if s <= self.t.first || !(ascii_space(c[s - 1]) || c[s - 1] == ')') {
            return None;
        }
        [("and", false), ("or", true)].into_iter().find_map(|(word, or)| {
            let after = c.get(s + word.len());
            (starts(c, s, word)
                && after.is_some_and(|&a| ascii_space(a) || a == '('))
            .then_some((or, word.len()))
        })
    }

    fn item(&mut self) -> Result<Node, Halt> {
        let (c, start) = (&self.t.c, self.pos);
        let after_space = start == 0 || {
            let before = c[start - 1];
            ascii_space(before) || before == '(' || before == ')'
        };
        let not = after_space
            && starts(c, start, "not")
            && c.get(start + 3).is_some_and(|&a| ascii_space(a));
        if not {
            self.pos += 3;
            while c.get(self.pos).is_some_and(|c| c.is_whitespace()) {
                self.pos += 1;
            }
            if c.get(self.pos) == Some(&'(') {
                return Err(Halt::Reject);
            }
        }
        let filter = self.filter(not.then_some(start))?;
        Ok(filter.unwrap_or_else(|| self.pattern(start, not)))
    }

    /// A filter at the current position; after `not` at `not`, a negated
    /// one. `None` when there is none, or when a quoted value after `not`
    /// is malformed, which leaves the text after the `:` to read.
    fn filter(&mut self, not: Option<usize>) -> Result<Option<Node>, Halt> {
        let (t, s) = (self.t, self.pos);
        let Some(&first) = t.c.get(s) else { return Ok(None) };
        let dash = first == '-';
        let letters = s + usize::from(dash);
        let colon = t.letters[letters] as usize;
        if colon == letters
            || t.c.get(colon) != Some(&':')
            || !known(&t.c[letters..colon])
        {
            return Ok(None);
        }
        let field = t.text(letters, colon);
        let v = colon + 1;
        let (value, end, predicate) = if matches!(t.c.get(v), Some('"' | '\''))
        {
            let close = t.closing(v);
            match close.and_then(|close| Some((t.unquote(v, close)?, close))) {
                Some((value, close)) => (value, close + 1, false),
                None if not.is_some() => {
                    self.pos = v;
                    return Ok(None);
                }
                None => return Err(Halt::Reject),
            }
        } else if let Some(end) = t.predicate(&field, v) {
            (t.text(v, end), end, true)
        } else {
            let end = t.run(v).unwrap_or_else(|| t.plain(v, true, true));
            (t.text(v, end), end, false)
        };
        if dash && not.is_some() {
            return Err(Halt::Reject);
        }
        self.pos = end;
        let negated = dash || not.is_some();
        let at = not.unwrap_or(s);
        let filter = Filter { field, value, negated, predicate, at };
        Ok(Some(Node::Filter(Box::new(filter))))
    }

    fn pattern(&mut self, at: usize, negated: bool) -> Node {
        let (t, s, ty) = (self.t, self.pos, self.ty);
        let slashes =
            !matches!(ty, PatternType::Literal | PatternType::Structural);
        let quotes = matches!(ty, PatternType::Keyword | PatternType::Regexp);
        let close = match t.c.get(s) {
            Some('/') if slashes => t.closing(s),
            Some('"' | '\'') if quotes => t.closing(s),
            _ => None,
        };
        if let Some(c) = close.filter(|&c| t.terminal(c + 1)) {
            self.pos = c + 1;
            let slash = t.c[s] == '/';
            return pattern(
                at,
                negated,
                c == s + 1 && !slash,
                slash && c > s + 1,
            );
        }
        self.pos = match (!self.flat).then(|| t.run(s)).flatten() {
            Some(end) => end,
            None if ty == PatternType::Regexp => t.plain(s, !self.flat, true),
            None => t.plain(s, false, false),
        };
        pattern(at, negated, self.pos == s, false)
    }
}

#[cfg(test)]
mod tests;
