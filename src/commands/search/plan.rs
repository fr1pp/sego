//! The basic queries Sourcegraph plans for a query sent with `patternType:
//! keyword`, each with its filters. `plan/examples.jsonl` holds the plans a
//! Sourcegraph server gives a corpus of queries, and this must match each.

use std::collections::{VecDeque, vec_deque};
use std::mem::take;

mod read;

use read::{Text, read};

/// The fields the server reads as filters, in any case.
pub const KNOWN: &[&str] = &[
    "repo",
    "r",
    "file",
    "f",
    "path",
    "lang",
    "l",
    "language",
    "content",
    "type",
    "case",
    "fork",
    "archived",
    "visibility",
    "rev",
    "revision",
    "context",
    "g",
    "patterntype",
    "select",
    "count",
    "timeout",
    "index",
    "before",
    "after",
    "since",
    "until",
    "author",
    "committer",
    "message",
    "m",
    "msg",
    "repohasfile",
    "repohascommitafter",
    "rule",
];

/// The most basic queries the rules that read each one's filters take; a
/// plan of 64 or more is unread to them.
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

/// A filter of a basic query: the field lower-cased with its aliases
/// resolved, and the value as the server reads it.
#[derive(Clone, Debug)]
pub struct Filter {
    pub field: String,
    pub value: String,
    pub negated: bool,
    pub predicate: bool,
    at: usize,
}

/// What a query writes, in the order written: each filter of a known field
/// and each pattern, as the server reads them apart, and each `or` it reads
/// as an operator. A filter with an empty value, which the server drops, is
/// left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    Filter(WrittenFilter),
    /// A pattern's text, without a `not` before it, which negates it.
    /// `delimited` is a quoted or `/`-delimited pattern.
    Pattern {
        text: String,
        negated: bool,
        delimited: bool,
    },
    Or,
}

/// A filter as written: the field as spelled and the value with any quotes.
/// `negated` is a `-` before it or a `not`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrittenFilter {
    pub negated: bool,
    pub field: String,
    pub value: String,
}

impl WrittenFilter {
    /// The filter as the query could restate it.
    pub fn raw(&self) -> String {
        format!("{}{}:{}", if self.negated { "-" } else { "" }, self.field, self.value)
    }
}

/// The values of the non-negated `name:` filters; a negated `type:` or
/// `select:` doesn't choose what the results are.
pub fn chosen<'a>(written: &'a [Written], name: &'a str) -> impl Iterator<Item = &'a str> {
    written.iter().filter_map(move |item| match item {
        Written::Filter(f) if !f.negated && f.field.eq_ignore_ascii_case(name) => {
            Some(f.value.as_str())
        }
        _ => None,
    })
}

/// Whether the query writes a pattern.
pub fn has_pattern(written: &[Written]) -> bool {
    written.iter().any(|item| matches!(item, Written::Pattern { .. }))
}

/// A quoted value without its quotes.
pub fn unquoted(value: &str) -> &str {
    ['"', '\''].iter().find_map(|q| value.strip_prefix(*q)?.strip_suffix(*q)).unwrap_or(value)
}

pub enum Plan {
    Read(BasicQueries),
    Rejected,
    /// A query of 4 Gi characters or more, past the narrower indices that
    /// keep a reading small.
    Unread,
}

pub struct BasicQueries {
    pub pattern_type: PatternType,
    root: Option<Node>,
    written: Vec<Written>,
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
        return Plan::Read(BasicQueries { pattern_type: ty, root: None, written: Vec::new() });
    }
    // Narrower indices keep the reading small; no command line comes near
    // 4 Gi characters.
    let Some(text) = Text::new(query) else { return Plan::Unread };
    let literal = read(&text, PatternType::Literal);
    if let Ok(reading) = &literal {
        for f in reading.root.filters() {
            if f.field.eq_ignore_ascii_case("patterntype") {
                ty = PatternType::named(&f.value).unwrap_or(ty);
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
    Plan::Read(BasicQueries { pattern_type: ty, root: Some(root), written: reading.written })
}

impl Plan {
    /// What the query writes, when the plan is read; nothing otherwise.
    pub fn written(&self) -> &[Written] {
        match self {
            Plan::Read(basic_queries) => &basic_queries.written,
            _ => &[],
        }
    }

    /// Each basic query's filters, unless the plan isn't read or holds 64
    /// basic queries or more.
    pub fn each(&self) -> Option<Vec<Vec<&Filter>>> {
        match self {
            Plan::Read(basic_queries) => basic_queries.list(MAX_LISTED),
            _ => None,
        }
    }
}

impl BasicQueries {
    /// How many basic queries the plan holds, saturating at `u64::MAX`.
    pub fn count(&self) -> u64 {
        self.root.as_ref().map_or(0, Node::count)
    }

    /// Each basic query's filters, in the server's order, when there are at
    /// most `limit` basic queries.
    pub fn list<'a>(&'a self, limit: usize) -> Option<Vec<Vec<&'a Filter>>> {
        if self.count() > u64::try_from(limit).unwrap_or(u64::MAX) {
            return None;
        }
        let step = |mut sofar: Vec<&'a Filter>, f: &'a Filter| {
            sofar.push(f);
            sofar
        };
        let root = self.root.as_ref();
        Some(root.map_or_else(Vec::new, |r| r.walk(vec![vec![]], &step, &|ends| ends)))
    }

    /// The states that folding `step` over each basic query's filters, from
    /// `start`, ends in, for every basic query however many there are.
    pub fn reach<S: Clone + PartialEq>(&self, start: S, step: impl Fn(S, &Filter) -> S) -> Vec<S> {
        self.root.as_ref().map_or_else(Vec::new, |root| root.walk(vec![start], &step, &distinct))
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
    if all.items.len() == 1
        && let Some(only) = all.items.pop_front()
    {
        return only;
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

    /// The basic queries, saturating at `u64::MAX`: an `or` with a filter
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

    /// The states folding `step` over each basic query's filters ends in, in
    /// the order of the basic queries, `settle` applied to the states each item
    /// ends in.
    fn walk<'a, S: Clone>(
        &'a self,
        states: Vec<S>,
        step: &impl Fn(S, &'a Filter) -> S,
        settle: &impl Fn(Vec<S>) -> Vec<S>,
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
                Node::Filter(f) => Some(states.drain(..).map(|s| step(s, f.as_ref())).collect()),
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
                    let found = settle(found);
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
                ends = Some(if op.kind == Kind::Or { done.ends } else { done.states });
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
    let mut frames: Vec<(Kind, vec_deque::IntoIter<Node>, Vec<Node>)> = Vec::new();
    let mut node = root;
    loop {
        let mut value = match node {
            Node::Op(mut op) => {
                frames.push((op.kind, take(&mut op.run.items).into_iter(), Vec::new()));
                None
            }
            node => Some(leaf(node)),
        };
        loop {
            let Some((_, items, done)) = frames.last_mut() else {
                let Some(value) = value else { unreachable!() };
                return value;
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
            let filters = |lead: &VecDeque<Node>| lead.iter().all(|n| matches!(n, Node::Filter(_)));
            match lead.pop_back() {
                Some(Node::Op(mut or)) if or.kind == Kind::Or && filters(&lead) => {
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
    let (Some(first), Some(last)) = (operands.pop_front(), operands.pop_back()) else {
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
    let (filters, rest): (Vec<Node>, Vec<Node>) =
        take(&mut op.run.items).into_iter().partition(|n| matches!(n, Node::Filter(_)));
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
        (op.kind == Kind::And && span.0 != usize::MAX).then_some((filters, span))
    };
    let (Some(first), Some(last)) = (operands.front(), operands.back()) else {
        return false;
    };
    let (Some((lead, (start, _))), Some((trail, (_, end)))) = (split(first), split(last)) else {
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
                Node::Op(mut seq) if seq.kind == Kind::Seq && kind == Kind::Seq => {
                    all.push(combine(Kind::And, take(&mut seq.run.items)));
                }
                Node::Op(mut seq) if seq.kind == Kind::Seq => {
                    all.extend(take(&mut seq.run.items));
                }
                item => all.push(item),
            }
        }
        if kind == Kind::Seq { op(Kind::Seq, spill(all, ty)) } else { combine(kind, all) }
    };
    let leaf = |node| match node {
        Node::Filter(f) => renamed(*f),
        node => node,
    };
    match rebuild(root, &leaf, &build) {
        Node::Op(mut seq) if seq.kind == Kind::Seq => combine(Kind::And, take(&mut seq.run.items)),
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
        if let (Some(Node::Pattern(last)), Node::Pattern(p)) = (out.last_mut(), &item)
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use crate::sourcegraph::{DEADLINE, Endpoint, GRAPHQL_PATH};

    const EXAMPLES: &str = include_str!("plan/examples.jsonl");
    const PAUSE: Duration = Duration::from_secs(1);

    const PARSE_TREE: &str = "\
query ParseTree($query: String!, $patternType: SearchPatternType!) {
  parseSearchQuery(query: $query, patternType: $patternType, outputPhase: PARSE_TREE, outputFormat: JSON)
}
";

    /// sourcegraph.com's parse tree of `query` read as `pattern_type`.
    fn parse_tree(query: &str, pattern_type: &str) -> Value {
        let body = json!({
            "query": PARSE_TREE,
            "variables": {"query": query, "patternType": pattern_type},
            "operationName": "ParseTree",
        });
        let transport = crate::connect(Endpoint::default());
        let response = transport
            .post(GRAPHQL_PATH, body.to_string().as_bytes(), Instant::now() + DEADLINE)
            .unwrap_or_else(|e| panic!("{query}: {e:?}"));
        let answered: Value =
            serde_json::from_slice(&response.body).unwrap_or_else(|e| panic!("{query}: {e}"));
        let tree = answered["data"]["parseSearchQuery"]
            .as_str()
            .unwrap_or_else(|| panic!("{query}: {answered}"));
        serde_json::from_str(tree).unwrap_or_else(|e| panic!("{query}: {e}"))
    }

    /// A plan as one line: `rejected`, `unread`, or the pattern type, the number
    /// of basic queries, and each listed one's filters in the fixture's notation.
    /// With `revisions`, a basic query's `rev:` is moved onto its `repo:` values,
    /// as the server does once its checks on the basic query pass.
    fn render(query: &str, revisions: bool) -> String {
        let basic_queries = match plan(query) {
            Plan::Rejected => return "rejected".to_owned(),
            Plan::Unread => return "unread".to_owned(),
            Plan::Read(basic_queries) => basic_queries,
        };
        let count = basic_queries.count();
        if let Some(list) = basic_queries.list(4096) {
            let mut each: Vec<Vec<String>> = Vec::new();
            for basic in list {
                let basic: Vec<String> = basic.into_iter().map(notation).collect();
                if !each.contains(&basic) {
                    each.push(basic);
                }
            }
            let reached = basic_queries.reach(Vec::new(), |mut sofar: Vec<String>, f| {
                sofar.push(notation(f));
                sofar
            });
            let same =
                reached.len() == each.len() && reached.iter().all(|basic| each.contains(basic));
            assert!(same, "{query:?}: reach and list differ");
        }
        let listed = basic_queries.list(4096).map(|list| {
            let list: Vec<String> = list
                .into_iter()
                .map(|filters| {
                    let mut filters: Vec<Filter> = filters.into_iter().cloned().collect();
                    if revisions {
                        revise(&mut filters);
                    }
                    let filters: Vec<String> = filters.iter().map(notation).collect();
                    encoded(&filters.join(" "))
                })
                .collect();
            list.join("  ||  ")
        });
        let ty = basic_queries.pattern_type.name();
        format!("{ty} {count} | {}", listed.unwrap_or_default())
    }

    fn revise(filters: &mut Vec<Filter>) {
        let Some(rev) = filters.iter().find(|f| f.field == "rev") else {
            return;
        };
        let rev = if rev.predicate { "rat=…".to_owned() } else { rev.value.clone() };
        filters.retain(|f| f.field != "rev");
        for f in filters.iter_mut() {
            if f.field == "repo" && !f.negated && !f.predicate {
                f.value = format!("{}@{rev}", f.value);
            }
        }
    }

    /// `field:value`, with `-` before a negated filter and ` [p]` after a
    /// predicate.
    fn notation(f: &Filter) -> String {
        let dash = if f.negated { "-" } else { "" };
        let mark = if f.predicate { " [p]" } else { "" };
        format!("{dash}{}:{}{mark}", f.field, f.value)
    }

    /// A revision the server encodes, from `rev:at.time(…)`, written `rat=…`.
    fn encoded(filters: &str) -> String {
        let mut out = String::new();
        for (i, part) in filters.split("@rat=").enumerate() {
            if i > 0 {
                out.push_str("@rat=…");
            }
            let part = if i > 0 { &part[part.find(' ').unwrap_or(part.len())..] } else { part };
            out.push_str(part);
        }
        out
    }

    /// The server's plan as `render` writes sego's, and whether the server's
    /// checks passed, so that it moved `rev:` filters.
    fn expected(row: &Value) -> (String, bool) {
        let text = |at: usize| row[at].as_str().unwrap_or_default().to_owned();
        if row[1] == "rejected" || row[1] == "unread" {
            return (text(1), true);
        }
        let listed = row[3].as_array().map(|list| {
            let list = list.iter().map(|basic| {
                let filters = basic.as_array().into_iter().flatten();
                let filters: Vec<&str> = filters.filter_map(Value::as_str).collect();
                encoded(&filters.join(" "))
            });
            list.collect::<Vec<_>>().join("  ||  ")
        });
        let line = format!("{} {} | {}", text(1), row[2], listed.unwrap_or_default());
        (line, row[4] != "rejected by checks")
    }

    /// The recorded rows whose plan sego's differs from, and the number of rows
    /// compared.
    fn disagreements() -> (usize, Vec<String>) {
        let mut found = Vec::new();
        let mut rows = 0;
        for line in EXAMPLES.lines() {
            let row: Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}"));
            rows += 1;
            let query = row[0].as_str().unwrap_or_default();
            let (theirs, revised) = expected(&row);
            let mine = render(query, revised);
            if mine != theirs {
                found.push(format!("{query:?}\n  sego:   {mine}\n  server: {theirs}"));
            }
        }
        (rows, found)
    }

    /// A few rows of `examples.jsonl` for each kind of
    /// planning rule, written out.
    const RULE_EXAMPLES: &[(&str, &str)] = &[
        ("repo:x foo or bar file:y", "keyword 1 | repo:x file:y"),
        ("foo or bar repo:x", "keyword 2 |   ||  repo:x"),
        ("x or ", "rejected"),
        ("patterntype:regexp foo", "regexp 1 | patterntype:regexp"),
        ("PatternType:regexp foo", "regexp 1 | patterntype:regexp"),
        ("patterntype:Regexp foo", "keyword 1 | patterntype:Regexp"),
        ("repo:x foo:bar", "keyword 1 | repo:x"),
        ("FILE:a Lang:go foo", "keyword 1 | file:a lang:go"),
        (r#"repo:"a b" foo"#, "keyword 1 | repo:a b"),
        (r#"repo:'it\'s' foo"#, "keyword 1 | repo:it's"),
        (r#"repo:"a\q" foo"#, "keyword 1 | repo:a\\q"),
        (r#"repo:"a\'b" foo"#, "keyword 1 | repo:a\\'b"),
        ("repo:", "keyword 1 | repo:"),
        ("repo: file:", "keyword 1 | file:"),
        ("repo:x a OR b", "keyword 1 | repo:x"),
        ("repo:x a Or(b)", "keyword 2 | repo:x  ||  "),
        ("repo:x a\tor\tb", "keyword 1 | repo:x"),
        ("not repo:x foo", "keyword 1 | -repo:x"),
        ("not -repo:x foo", "rejected"),
        ("not (foo)", "rejected"),
        (r#"patterntype:standard "a b""#, "standard 1 | patterntype:standard"),
        ("patterntype:regexp 'a b'", "regexp 1 | patterntype:regexp"),
        ("patterntype:literal /a b/", "literal 1 | patterntype:literal"),
        ("repo:x (a b) or c", "keyword 2 | repo:x  ||  "),
        ("repo:x patterntype:literal (a b) or c", "literal 1 | repo:x patterntype:literal"),
        (
            "repo:x patterntype:literal (a or b) or c",
            "literal 2 | repo:x patterntype:literal  ||  ",
        ),
        ("x (a)or b) c", "rejected"),
        ("repo:y (repo:x a)or b", "keyword 1 | repo:y repo:x"),
        ("foo)", "rejected"),
        ("x )", "rejected"),
        (r#"patterntype:regexp "(" x)"#, "rejected"),
        ("repo:x patterntype:literal (a or\tb) or c", "literal 1 | repo:x patterntype:literal"),
        ("repo:x patterntype:literal (a xfile:x) or c", "literal 1 | repo:x patterntype:literal"),
        (
            "repo:x patterntype:literal (a file:) or c",
            "literal 2 | repo:x patterntype:literal  ||  ",
        ),
        ("(file:b bar) file:a foo", "keyword 1 | file:a file:b"),
        ("foo file:a (bar file:b)", "keyword 1 | file:a file:b"),
        ("(file:b) foo file:a", "keyword 1 | file:b file:a"),
        (
            "repo:x file:y and (file:z foo or bar)",
            "keyword 2 | repo:x file:y file:z  ||  repo:x file:y",
        ),
        ("repo:x file:y (file:z foo or bar)", "keyword 1 | repo:x file:y file:z"),
        ("repo:x and (file:z foo or bar)", "keyword 1 | repo:x file:z"),
        ("repo:x (foo or bar", "keyword 1 | repo:x"),
        ("repo:x (foo or bar) or baz (", "keyword 1 | repo:x"),
        ("x or or y", "rejected"),
        ("repo:x foo or bar", "keyword 1 | repo:x"),
        (" foo or bar repo:x", "keyword 1 | repo:x"),
        ("(foo or bar repo:x)", "keyword 1 | repo:x"),
        ("patterntype:literal foo (content:bar baz)", "keyword 1 | patterntype:literal"),
        (
            "repo:x patterntype:literal (a or b)c or d",
            "keyword 2 | repo:x patterntype:literal  ||  ",
        ),
        ("patterntype:standard /x (/ (a or b)", "rejected"),
        (
            "r:x f:y path:z l:go language:rust foo",
            "keyword 1 | repo:x file:y file:z lang:go lang:rust",
        ),
        (
            "type:commit since:2020 until:2021 m:a msg:b foo",
            "keyword 1 | type:commit after:2020 before:2021 message:a message:b",
        ),
        ("f:has.content(x) foo", "keyword 1 | file:has.content(x) [p]"),
        (r#"patterntype:regexp (x or repo:y z) w "a (b""#, "regexp 1 | patterntype:regexp"),
        (
            r#"patterntype:regexp repo:q (x or repo:y z) w "a (b""#,
            "regexp 1 | patterntype:regexp repo:q",
        ),
        (
            r#"a (file:y b) or c "q (z" patterntype:regexp"#,
            "regexp 3 |   ||  file:y  ||  patterntype:regexp",
        ),
        (
            "(repo:a or repo:b) (file:x or file:y) foo",
            "keyword 4 | repo:a file:x  ||  repo:b file:x  ||  repo:a file:y  ||  repo:b file:y",
        ),
        ("repo:x (a or file:y b) c", "keyword 2 | repo:x  ||  repo:x file:y"),
        (r#"(repo:x or repo:y) ("a" or "b")"#, "keyword 2 | repo:x  ||  repo:y"),
        ("repo:x rev:v1 foo", "keyword 1 | repo:x@v1"),
        ("repo:x repo:y rev:v1 foo", "keyword 1 | repo:x@v1 repo:y@v1"),
        (
            "repo:x -repo:y repo:has.file(z) rev:v1 foo",
            "keyword 1 | repo:x@v1 -repo:y repo:has.file(z) [p]",
        ),
        ("count:5 foo or count:6 bar", "keyword 2 | count:5  ||  count:6"),
        ("type:commit author:bob foo", "keyword 1 | type:commit author:bob"),
        ("index:False foo", "keyword 1 | index:False"),
    ];

    #[test]
    fn the_plan_of_each_rule_example_renders_as_documented() {
        for &(query, expected) in RULE_EXAMPLES {
            assert_eq!(render(query, true), expected, "{query:?}");
        }
    }

    #[test]
    fn an_empty_query_plans_no_basic_queries() {
        for query in ["", "  \t"] {
            let Plan::Read(basic_queries) = plan(query) else { panic!("{query:?}") };
            assert_eq!(basic_queries.count(), 0);
            assert_eq!(basic_queries.list(1).map(|list| list.len()), Some(0));
            assert_eq!(basic_queries.reach(0, |n, _| n + 1), Vec::<i32>::new());
        }
    }

    fn pairs(n: usize) -> String {
        let groups: Vec<String> = (0..n).map(|i| format!("(repo:a{i} or repo:b{i})")).collect();
        format!("{} foo", groups.join(" "))
    }

    #[test]
    fn a_plan_lists_its_basic_queries_only_when_they_fit_the_size_asked() {
        let Plan::Read(basic_queries) = plan(&pairs(6)) else { panic!() };
        assert_eq!(basic_queries.count(), 64);
        assert_eq!(basic_queries.list(64).map(|list| list.len()), Some(64));
        assert!(basic_queries.list(63).is_none());
        assert_eq!(plan(&pairs(5)).each().map(|each| each.len()), Some(32));
        assert!(plan(&pairs(6)).each().is_none());
    }

    #[test]
    fn reach_covers_every_basic_query_however_many() {
        let Plan::Read(basic_queries) = plan(&pairs(70)) else { panic!() };
        assert_eq!(basic_queries.count(), u64::MAX);
        assert!(basic_queries.list(4096).is_none());
        let ends = basic_queries.reach(0, |n, f| n + usize::from(f.value.starts_with('b')));
        assert_eq!(ends.len(), 71);
    }

    /// The server's plans for 120,000 nested groups, in keyword mode and as one
    /// balanced run in literal mode, read on a small stack. These two are built
    /// here rather than stored in the fixture.
    #[test]
    fn groups_nest_at_any_depth() {
        let nested = |depth, inner| format!("{}{inner}{}", "(".repeat(depth), ")".repeat(depth));
        let rows = [
            (nested(64, "repo:x foo"), "keyword 1 | repo:x"),
            (nested(65, "repo:x foo"), "keyword 1 | repo:x"),
            (nested(120_000, "repo:x foo"), "keyword 1 | repo:x"),
            (nested(120_000, "foo"), "keyword 1 | "),
            (
                format!("patterntype:literal {}", nested(120_000, "foo")),
                "literal 1 | patterntype:literal",
            ),
        ];
        let run = move || {
            for (query, expected) in rows {
                assert_eq!(render(&query, true), expected);
            }
        };
        let thread = std::thread::Builder::new().stack_size(256 << 10).spawn(run);
        let joined = thread.map(|t| t.join().is_ok());
        assert!(joined.is_ok_and(|ok| ok));
    }

    /// `)or` keeps a closed group's or-expression open, so a plan can nest far
    /// deeper than its groups; sego reads it at any nesting, on a small stack.
    #[test]
    fn or_expressions_plan_at_any_nesting_on_a_small_stack() {
        let chain = |n| format!("{}y", "(a)or x ".repeat(n));
        let mebibyte = chain((1 << 20) / 8);
        let run = move || {
            let within = format!("{}{}", "(".repeat(63), chain(20_000));
            for query in [chain(601), chain(1_000), mebibyte, within] {
                let Plan::Read(basic_queries) = plan(&query) else { panic!() };
                assert_eq!(basic_queries.list(1).map(|list| list.len()), Some(1));
            }
        };
        let thread = std::thread::Builder::new().stack_size(256 << 10).spawn(run);
        let joined = thread.map(|t| t.join().is_ok());
        assert!(joined.is_ok_and(|ok| ok));
    }

    /// Plans hundreds of levels deep are planned, listed and walked on a small
    /// stack: none of it recurses.
    #[test]
    fn deep_plans_need_little_stack() {
        let queries = [
            format!("{}y", "(a)or x ".repeat(600)),
            format!("{}y", "(repo:a a)or x and file:b y ".repeat(600)),
            format!("{}y", "(a)or x (b) ".repeat(599)),
            format!("{}{}y{}", "(".repeat(63), "(a)or x ".repeat(537), ")".repeat(63)),
            format!("patterntype:regexp {}y", "(a)or x (b) ".repeat(599)),
        ];
        let run = move || {
            for query in &queries {
                let Plan::Read(basic_queries) = plan(query) else { panic!("{query}") };
                basic_queries.list(4096);
                basic_queries.reach(0, |n, _| n + 1);
            }
        };
        let thread = std::thread::Builder::new().stack_size(256 << 10).spawn(run);
        assert!(
            thread.and_then(|t| t.join().map_err(|_| std::io::ErrorKind::Other.into())).is_ok()
        );
    }

    /// Shapes that combine the most items for their length, at 2,048 bytes,
    /// with the server's plans.
    #[test]
    fn plans_the_costliest_queries_of_2_048_bytes() {
        let fill = |head: &str, unit: &str| {
            let mut query = format!("{head}{}", unit.repeat(2048 / unit.len()));
            query.truncate(2048);
            query
        };
        let shapes = [
            ("", "(a b ", "keyword 1 | "),
            ("", "a or (", "keyword 1 | "),
            ("", "x and (a ", "keyword 1 | "),
            ("", "(", "keyword 1 | "),
            ("", "(a)or x ", "keyword 1 | "),
            ("", "a (b or ", "rejected"),
            ("", "(a (b ", "keyword 1 | "),
            ("patterntype:regexp ", "a(", "regexp 1 | patterntype:regexp"),
        ];
        for (head, unit, expected) in shapes {
            let query = fill(head, unit);
            assert_eq!(render(&query, true), expected, "{head}{unit:?}");
        }
    }

    /// One basic query can hold a filter for every few bytes; listing it, as
    /// `Query::new` does for `index:no`, gives each of them once.
    #[test]
    fn one_basic_query_lists_every_filter_of_a_mebibyte_query() {
        let query = "f:a ".repeat((1 << 20) / 4);
        let filters = plan(&query).each().map(|each| each[0].len());
        assert_eq!(filters, Some(1 << 18));
    }

    /// Every recorded plan of the server agrees, and there is at least one.
    #[test]
    fn every_recorded_plan_of_the_server_agrees_with_ours() {
        let (rows, found) = disagreements();
        assert!(rows > 0, "no plans to compare");
        assert!(
            found.is_empty(),
            "{} of {rows} plans differ\n{}",
            found.len(),
            found.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
        );
    }

    /// Queries from `examples.jsonl`, across the plan's
    /// rules, in the three modes sourcegraph.com can be asked to read a query
    /// in. Keyword mode can't be checked this way: `parseSearchQuery` reads a
    /// query sent as keyword as literal.
    const PLANNED: [&str; 20] = [
        "repo:x patterntype:literal (a or b) or c",
        "repo:x patterntype:literal (a -file:x) or c",
        "repo:x patterntype:literal (\"a or b\") or baz",
        "repo:x patterntype:regexp (\"b or c\") or d",
        "a (file:y b) or c \"q (z\" patterntype:regexp",
        "patterntype:regexp (\"x(\" b) (c or repo:d e) \"q (z\"",
        "repo:x patterntype:regexp (foo and bar) or baz",
        "patterntype:structural foo(...) or (lang:go bar(...))",
        r"repo:x patterntype:structural (file:^a\.go$ foo(...) or file:^b\.go$ bar(...))",
        r"foo(...) or bar(...) file:^a\.go$ patterntype:structural",
        r"repo:x not file:y file:^a\.go$ patterntype:structural foo(...) or bar(...)",
        r"repo:x file:has.content(x) file:\.go$ patterntype:structural foo(...)",
        r"repo:x l:go file:^Makefile$ patterntype:structural foo(...)",
        "patterntype:regexp (not repo:x foo)",
        "patterntype:regexp (repo:y a)not repo:x",
        "patterntype:regexp (repo:c (\"\" and repo:a repo:b)) repo:z",
        "patterntype:regexp repo:a or \"\"",
        "patterntype:regexp repo:a or \"\" foo",
        "patterntype:standard repo:a or // foo",
        r#"patterntype:regexp repo:"a\nb\rc" foo"#,
    ];

    /// The basic queries a parse tree from `parseSearchQuery` implies, each filter
    /// written `-field:value`, with ` [p]` after a predicate: an `or` with a
    /// filter inside splits, and an `and` multiplies its items' basic queries.
    fn tree_basic_queries(node: &Value) -> Vec<Vec<String>> {
        fn holds_filter(node: &Value) -> bool {
            match node {
                Value::Array(items) => items.iter().any(holds_filter),
                Value::Object(map) if map.contains_key("field") => true,
                Value::Object(map) => map.values().any(holds_filter),
                _ => false,
            }
        }
        let and = |items: &[Value]| {
            let mut basic_queries = vec![Vec::new()];
            for item in items {
                let mut next = Vec::new();
                for own in tree_basic_queries(item) {
                    for sofar in &basic_queries {
                        next.push([sofar.clone(), own.clone()].concat());
                    }
                }
                basic_queries = next;
            }
            basic_queries
        };
        match node {
            Value::Array(items) => and(items),
            Value::Object(map) => {
                if let Some(Value::Array(items)) = map.get("and") {
                    and(items)
                } else if let Some(Value::Array(items)) = map.get("or") {
                    if holds_filter(node) {
                        items.iter().flat_map(tree_basic_queries).collect()
                    } else {
                        vec![Vec::new()]
                    }
                } else if let Some(field) = map.get("field").and_then(Value::as_str) {
                    let dash = if map["negated"] == true { "-" } else { "" };
                    let value = map["value"].as_str().unwrap_or_default();
                    let labels = map["labels"].as_array().into_iter().flatten();
                    let predicate = labels.into_iter().any(|l| l == "IsPredicate");
                    let mark = if predicate { " [p]" } else { "" };
                    vec![vec![format!("{dash}{field}:{value}{mark}")]]
                } else {
                    vec![Vec::new()]
                }
            }
            _ => vec![Vec::new()],
        }
    }

    /// sourcegraph.com's parse tree for each query of `PLANNED`, asked for one
    /// at a time in the query's mode, implies the basic queries and filters the plan
    /// module gives it.
    #[test]
    #[ignore = "sends requests to sourcegraph.com"]
    fn plans_match_the_live_server() {
        let mut failures = Vec::new();
        for query in PLANNED {
            let Plan::Read(basic_queries) = plan(query) else { panic!("{query}") };
            let mode = basic_queries.pattern_type.name();
            let mine: Vec<Vec<String>> = basic_queries
                .list(4096)
                .unwrap_or_default()
                .into_iter()
                .map(|filters| {
                    filters
                        .into_iter()
                        .map(|f| {
                            let dash = if f.negated { "-" } else { "" };
                            let mark = if f.predicate { " [p]" } else { "" };
                            format!("{dash}{}:{}{mark}", f.field, f.value)
                        })
                        .collect()
                })
                .collect();
            let theirs = tree_basic_queries(&parse_tree(query, mode));
            if mine != theirs {
                failures.push(format!("{query}\n  sego:   {mine:?}\n  server: {theirs:?}"));
            }
            sleep(PAUSE);
        }
        assert!(
            failures.is_empty(),
            "{} of {} plans differ from the server's parse tree:\n{}",
            failures.len(),
            PLANNED.len(),
            failures.join("\n")
        );
    }
}
