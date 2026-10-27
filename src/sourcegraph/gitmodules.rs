//! The submodule URLs a commit's `.gitmodules` holds. Sourcegraph looks a
//! submodule's URL up by its path, while git names the section by the
//! submodule's name, so the server's URL is empty wherever the two differ.

use std::collections::HashMap;

/// Each submodule's URL by its path, from the text of `.gitmodules` in git's
/// config syntax. A submodule without both is left out.
pub fn urls(text: &str) -> HashMap<String, String> {
    let mut urls = HashMap::new();
    let mut in_submodule = false;
    let (mut path, mut url): (Option<String>, Option<String>) = (None, None);
    let mut close = |path: &mut Option<String>, url: &mut Option<String>| {
        if let (Some(path), Some(url)) = (path.take(), url.take()) {
            urls.insert(path, url);
        }
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[') {
            close(&mut path, &mut url);
            let name = section.split(['"', ']', ' ', '\t']).next().unwrap_or_default();
            in_submodule = name.eq_ignore_ascii_case("submodule");
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if !in_submodule {
            continue;
        }
        let value = config_value(value);
        match key.trim().to_ascii_lowercase().as_str() {
            "path" => path = Some(value),
            "url" => url = Some(value),
            _ => {}
        }
    }
    close(&mut path, &mut url);
    urls
}

/// A config value: trimmed, unquoted, with its escapes read and a comment
/// after it dropped.
fn config_value(raw: &str) -> String {
    let mut value = String::new();
    let mut quoted = false;
    let mut chars = raw.trim().chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' => match chars.next() {
                Some('t') => value.push('\t'),
                Some('n') => value.push('\n'),
                Some('b') => {
                    value.pop();
                }
                Some(other) => value.push(other),
                None => {}
            },
            '#' | ';' if !quoted => break,
            c => value.push(c),
        }
    }
    if quoted { value } else { value.trim_end().to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_submodule_url_is_found_by_its_path_not_its_name() {
        let text = "[submodule \"assets/syntaxes/Packages\"]\n\
                    \tpath = assets/syntaxes/01_Packages\n\
                    \turl = https://github.com/sublimehq/Packages/\n\
                    # a comment\n\
                    [submodule \"x\"]\n\
                    \tPath = \"with space/dir\" ; a trailing comment\n\
                    \tURL = git@github.com:o/r.git\n\
                    [core]\n\
                    \tpath = not/a/submodule\n\
                    \turl = x\n\
                    [submodule \"no url\"]\n\
                    \tpath = no-url\n\
                    [submodule \"quoted\"]\n\
                    \tpath = \"a \\\"b\\\" \\\\ c\"\n\
                    \turl = u # comment\n";
        let urls = urls(text);
        assert_eq!(urls.len(), 3, "{urls:?}");
        assert_eq!(
            urls.get("assets/syntaxes/01_Packages").map(String::as_str),
            Some("https://github.com/sublimehq/Packages/")
        );
        assert_eq!(urls.get("with space/dir").map(String::as_str), Some("git@github.com:o/r.git"));
        assert_eq!(urls.get("a \"b\" \\ c").map(String::as_str), Some("u"));
    }

    #[test]
    fn crlf_line_endings_and_an_empty_file_are_read() {
        let urls = urls("[submodule \"a\"]\r\n\tpath = a\r\n\turl = https://h/a\r\n");
        assert_eq!(urls.get("a").map(String::as_str), Some("https://h/a"));
        assert!(super::urls("").is_empty());
    }
}
