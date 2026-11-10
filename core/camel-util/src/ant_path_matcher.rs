//! Ant-style path pattern matching (`*`, `?` and `**`).
//!
//! Port of `org.apache.camel.util.AntPathMatcher`.

/// The default path separator.
pub const DEFAULT_PATH_SEPARATOR: &str = "/";

/// PathMatcher implementation for Ant-style path patterns.
///
/// Part of this mapping code has been kindly borrowed from Apache Ant and Spring Framework.
///
/// The mapping matches URLs using the following rules:
///
/// * `?` matches one character
/// * `*` matches zero or more characters
/// * `**` matches zero or more 'directories' in a path
///
/// Some examples:
///
/// * `com/t?st.jsp` - matches `com/test.jsp` but also `com/tast.jsp` or `com/txst.jsp`
/// * `com/*.jsp` - matches all `.jsp` files in the `com` directory
/// * `com/**/test.jsp` - matches all `test.jsp` files underneath the `com` path
/// * `org/springframework/**/*.jsp` - matches all `.jsp` files underneath the
///   `org/springframework` path
/// * `org/**/servlet/bla.jsp` - matches `org/springframework/servlet/bla.jsp` but also
///   `org/springframework/testing/servlet/bla.jsp` and `org/servlet/bla.jsp`
///
/// # Examples
///
/// ```
/// use camel_util::ant_path_matcher::AntPathMatcher;
/// let matcher = AntPathMatcher::new();
/// assert!(matcher.match_("foo/**/*.txt", "foo/bar/blah.txt"));
/// assert!(!matcher.match_("*.txt", "foo/blah.txt"));
/// ```
#[derive(Debug, Clone)]
pub struct AntPathMatcher {
    path_separator: String,
}

impl Default for AntPathMatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared default instance (the Java `AntPathMatcher.INSTANCE`), using `/` as separator.
pub static INSTANCE: AntPathMatcher = AntPathMatcher {
    path_separator: String::new(),
};

impl AntPathMatcher {
    /// Creates a matcher using the [`DEFAULT_PATH_SEPARATOR`].
    pub fn new() -> Self {
        AntPathMatcher {
            path_separator: DEFAULT_PATH_SEPARATOR.to_string(),
        }
    }

    /// Sets the path separator to use.
    ///
    /// # Arguments
    ///
    /// * `sep` - the separator, `None` resets to the default [`DEFAULT_PATH_SEPARATOR`]
    pub fn set_path_separator(&mut self, sep: Option<&str>) {
        self.path_separator = sep.unwrap_or(DEFAULT_PATH_SEPARATOR).to_string();
    }

    fn sep(&self) -> &str {
        if self.path_separator.is_empty() {
            DEFAULT_PATH_SEPARATOR
        } else {
            &self.path_separator
        }
    }

    /// Whether the given path contains pattern characters (`*` or `?`).
    ///
    /// # Arguments
    ///
    /// * `path` - the path to check
    pub fn is_pattern(&self, path: &str) -> bool {
        path.contains('*') || path.contains('?')
    }

    /// Matches the given path against the given pattern (case sensitive).
    ///
    /// Named `match_` because `match` is a Rust keyword.
    ///
    /// # Arguments
    ///
    /// * `pattern` - the pattern to match against
    /// * `path` - the path string to test
    ///
    /// # Returns
    ///
    /// `true` if the supplied path matched, `false` if it didn't.
    pub fn match_(&self, pattern: &str, path: &str) -> bool {
        self.matches(pattern, path, true)
    }

    /// Matches the given path against the given pattern.
    ///
    /// # Arguments
    ///
    /// * `pattern` - the pattern to match against
    /// * `path` - the path string to test
    /// * `case_sensitive` - true if case sensitive, false if not
    ///
    /// # Returns
    ///
    /// `true` if the supplied path matched, `false` if it didn't.
    pub fn matches(&self, pattern: &str, path: &str, case_sensitive: bool) -> bool {
        self.do_match(pattern, path, true, case_sensitive)
    }

    /// Matches the given path against any of the given patterns.
    ///
    /// # Arguments
    ///
    /// * `patterns` - the patterns to match against
    /// * `path` - the path string to test
    /// * `case_sensitive` - true if case sensitive, false if not
    ///
    /// # Returns
    ///
    /// `true` if at least one pattern matched (`false` for an empty list).
    pub fn any_match<S: AsRef<str>>(
        &self,
        patterns: &[S],
        path: &str,
        case_sensitive: bool,
    ) -> bool {
        patterns
            .iter()
            .any(|p| self.matches(p.as_ref(), path, case_sensitive))
    }

    /// Matches the given path against the corresponding part of the given pattern, i.e. whether the
    /// pattern at least matches as far as the given base path goes, assuming that a full path may then
    /// match as well.
    ///
    /// # Arguments
    ///
    /// * `pattern` - the pattern to match against
    /// * `path` - the path string to test
    /// * `case_sensitive` - true if case sensitive, false if not
    ///
    /// # Returns
    ///
    /// `true` if the pattern matched the start of the path.
    pub fn match_start(&self, pattern: &str, path: &str, case_sensitive: bool) -> bool {
        self.do_match(pattern, path, false, case_sensitive)
    }

    fn do_match(&self, pattern: &str, path: &str, full_match: bool, cs: bool) -> bool {
        let sep = self.sep();
        if path.starts_with(sep) != pattern.starts_with(sep) {
            return false;
        }
        let patt_dirs = tokenize_to_string_array(pattern, sep);
        let path_dirs = tokenize_to_string_array(path, sep);

        let mut patt_start: isize = 0;
        let mut patt_end: isize = patt_dirs.len() as isize - 1;
        let mut path_start: isize = 0;
        let mut path_end: isize = path_dirs.len() as isize - 1;
        let pd = |i: isize| patt_dirs[i as usize].as_str();
        let sd = |i: isize| path_dirs[i as usize].as_str();

        // Match all elements up to the first **
        while patt_start <= patt_end && path_start <= path_end {
            let pat = pd(patt_start);
            if pat == "**" {
                break;
            }
            if !match_strings(pat, sd(path_start), cs) {
                return false;
            }
            patt_start += 1;
            path_start += 1;
        }

        if path_start > path_end {
            // Path is exhausted, only match if rest of pattern is * or **'s
            if patt_start > patt_end {
                return pattern.ends_with(sep) == path.ends_with(sep);
            }
            if !full_match {
                return true;
            }
            if patt_start == patt_end && pd(patt_start) == "*" && path.ends_with(sep) {
                return true;
            }
            return (patt_start..=patt_end).all(|i| pd(i) == "**");
        } else if patt_start > patt_end {
            return false;
        } else if !full_match && pd(patt_start) == "**" {
            return true;
        }

        // up to last '**'
        while patt_start <= patt_end && path_start <= path_end {
            let pat = pd(patt_end);
            if pat == "**" {
                break;
            }
            if !match_strings(pat, sd(path_end), cs) {
                return false;
            }
            patt_end -= 1;
            path_end -= 1;
        }
        if path_start > path_end {
            return (patt_start..=patt_end).all(|i| pd(i) == "**");
        }

        while patt_start != patt_end && path_start <= path_end {
            let mut pat_idx_tmp: isize = -1;
            for i in (patt_start + 1)..=patt_end {
                if pd(i) == "**" {
                    pat_idx_tmp = i;
                    break;
                }
            }
            if pat_idx_tmp == patt_start + 1 {
                // '**/**' situation, so skip one
                patt_start += 1;
                continue;
            }
            let pat_length = pat_idx_tmp - patt_start - 1;
            let str_length = path_end - path_start + 1;
            let mut found_idx: isize = -1;
            'str_loop: for i in 0..=(str_length - pat_length) {
                for j in 0..pat_length {
                    if !match_strings(pd(patt_start + j + 1), sd(path_start + i + j), cs) {
                        continue 'str_loop;
                    }
                }
                found_idx = path_start + i;
                break;
            }
            if found_idx == -1 {
                return false;
            }
            patt_start = pat_idx_tmp;
            path_start = found_idx + pat_length;
        }

        (patt_start..=patt_end).all(|i| pd(i) == "**")
    }

    /// Given a pattern and a full path, determine the pattern-mapped part.
    ///
    /// For example:
    ///
    /// * `/docs/cvs/commit.html` and `/docs/cvs/commit.html` -> `''`
    /// * `/docs/*` and `/docs/cvs/commit` -> `cvs/commit`
    /// * `/docs/cvs/*.html` and `/docs/cvs/commit.html` -> `commit.html`
    /// * `/docs/**` and `/docs/cvs/commit` -> `cvs/commit`
    /// * `/docs/**/*.html` and `/docs/cvs/commit.html` -> `cvs/commit.html`
    /// * `/*.html` and `/docs/cvs/commit.html` -> `docs/cvs/commit.html`
    /// * `*.html` and `/docs/cvs/commit.html` -> `/docs/cvs/commit.html`
    /// * `*` and `/docs/cvs/commit.html` -> `/docs/cvs/commit.html`
    ///
    /// Assumes that [`AntPathMatcher::match_`] returns `true` for the pattern and path, but does not
    /// enforce this.
    pub fn extract_path_within_pattern(&self, pattern: &str, path: &str) -> String {
        let sep = self.sep();
        let pattern_parts = tokenize_to_string_array(pattern, sep);
        let path_parts = tokenize_to_string_array(path, sep);
        let mut buffer = String::with_capacity(path.len());
        let mut puts = 0;
        for (i, part) in pattern_parts.iter().enumerate() {
            if (part.contains('*') || part.contains('?')) && path_parts.len() > i {
                if puts > 0 || (i == 0 && !pattern.starts_with(sep)) {
                    buffer.push_str(sep);
                }
                buffer.push_str(&path_parts[i]);
                puts += 1;
            }
        }
        for (i, part) in path_parts.iter().enumerate().skip(pattern_parts.len()) {
            if puts > 0 || i > 0 {
                buffer.push_str(sep);
            }
            buffer.push_str(part);
        }
        buffer
    }

    /// Determine the root directory for the given location.
    ///
    /// Used for determining the starting point for file matching, resolving the root directory
    /// location and passing the remainder of the location as pattern.
    ///
    /// Will return `/WEB-INF/` for the pattern `/WEB-INF/*.xml`, for example.
    ///
    /// # Arguments
    ///
    /// * `location` - the location to check
    ///
    /// # Returns
    ///
    /// The part of the location that denotes the root directory.
    pub fn determine_root_dir(&self, location: &str) -> String {
        let prefix_end = location.find(':').map(|p| p + 1).unwrap_or(0);
        let mut root_dir_end = location.len();
        while root_dir_end > prefix_end && self.is_pattern(&location[prefix_end..root_dir_end]) {
            // Java: location.lastIndexOf('/', rootDirEnd - 2) + 1
            root_dir_end = if root_dir_end < 2 {
                0
            } else {
                location.as_bytes()[..=root_dir_end - 2]
                    .iter()
                    .rposition(|&b| b == b'/')
                    .map(|p| p + 1)
                    .unwrap_or(0)
            };
        }
        if root_dir_end == 0 {
            root_dir_end = prefix_end;
        }
        location[..root_dir_end].to_string()
    }
}

/// Tokenize the given string into a list of strings.
///
/// Behaves like Java's `StringTokenizer`: any of the delimiter characters splits the string, each
/// token is trimmed and empty tokens are omitted.
///
/// # Arguments
///
/// * `s` - the string to tokenize
/// * `delimiters` - the delimiter characters
pub fn tokenize_to_string_array(s: &str, delimiters: &str) -> Vec<String> {
    s.split(|c: char| delimiters.contains(c))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect()
}

fn different(cs: bool, a: char, b: char) -> bool {
    if cs {
        a != b
    } else {
        a.to_uppercase().ne(b.to_uppercase())
    }
}

fn match_strings(pattern: &str, s: &str, cs: bool) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let st: Vec<char> = s.chars().collect();
    let mut pat_start: isize = 0;
    let mut pat_end: isize = pat.len() as isize - 1;
    let mut str_start: isize = 0;
    let mut str_end: isize = st.len() as isize - 1;
    let p = |i: isize| pat[i as usize];
    let c = |i: isize| st[i as usize];

    if !pat.contains(&'*') {
        if pat_end != str_end {
            return false;
        }
        return (0..=pat_end).all(|i| p(i) == '?' || !different(cs, p(i), c(i)));
    }
    if pat_end == 0 {
        return true; // only '*'
    }
    let only_stars = |from: isize, to: isize| (from..=to).all(|i| p(i) == '*');

    // characters before first star
    while p(pat_start) != '*' && str_start <= str_end {
        let ch = p(pat_start);
        if ch != '?' && different(cs, ch, c(str_start)) {
            return false;
        }
        pat_start += 1;
        str_start += 1;
    }
    if str_start > str_end {
        return only_stars(pat_start, pat_end);
    }

    // characters after last star
    while p(pat_end) != '*' && str_start <= str_end {
        let ch = p(pat_end);
        if ch != '?' && different(cs, ch, c(str_end)) {
            return false;
        }
        pat_end -= 1;
        str_end -= 1;
    }
    if str_start > str_end {
        return only_stars(pat_start, pat_end);
    }

    while pat_start != pat_end && str_start <= str_end {
        let mut pat_idx_tmp: isize = -1;
        for i in (pat_start + 1)..=pat_end {
            if p(i) == '*' {
                pat_idx_tmp = i;
                break;
            }
        }
        if pat_idx_tmp == pat_start + 1 {
            pat_start += 1;
            continue;
        }
        let pat_length = pat_idx_tmp - pat_start - 1;
        let str_length = str_end - str_start + 1;
        let mut found_idx: isize = -1;
        'str_loop: for i in 0..=(str_length - pat_length) {
            for j in 0..pat_length {
                let ch = p(pat_start + j + 1);
                if ch != '?' && different(cs, ch, c(str_start + i + j)) {
                    continue 'str_loop;
                }
            }
            found_idx = str_start + i;
            break;
        }
        if found_idx == -1 {
            return false;
        }
        pat_start = pat_idx_tmp;
        str_start = found_idx + pat_length;
    }
    only_stars(pat_start, pat_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let m = AntPathMatcher::new();
        assert!(m.match_("*.txt", "blah.txt"));
        assert!(!m.match_("*.txt", "foo/blah.txt"));
        assert!(m.match_("???.txt", "abc.txt"));
        assert!(m.match_("abc.t?t", "abc.tnt"));
        assert!(!m.match_("???.txt", "abcd.txt"));
        assert!(m.match_("**/*.txt", "blah.txt"));
        assert!(m.match_("**/*.txt", "foo/blah.txt"));
        assert!(m.match_("**/*.txt", "foo/bar/blah.txt"));
        assert!(m.match_("foo/**/*.txt", "foo/bar/blah.txt"));
        assert!(m.match_("foo/**/*.??", "foo/bar/blah.gz"));
        assert!(m.match_("foo/**/*.txt", "foo/blah.txt"));
        assert!(!m.match_("foo/**/*.txt", "blah/blah.txt"));
    }

    #[test]
    fn test_case_sensitive() {
        let m = AntPathMatcher::new();
        assert!(m.matches("foo/**/*.txt", "foo/blah.txt", true));
        assert!(m.matches("foo/**/*.txt", "foo/blah.txt", false));
        assert!(m.match_("foo/**/*.txt", "foo/BLAH.txt"));
        assert!(!m.match_("FOO/**/*.txt", "foo/blah.txt"));
        assert!(!m.match_("foo/**/*.TXT", "foo/blah.txt"));
        assert!(m.matches("foo/**/*.TXT", "foo/blah.txt", false));
        assert!(m.matches("FOO/**/*.txt", "foo/blah.txt", false));
        assert!(!m.matches("FOO/**/*.txt", "foo/blah.txt", true));
    }

    #[test]
    fn test_determine_root() {
        let m = AntPathMatcher::new();
        assert_eq!("org/apache/camel", m.determine_root_dir("org/apache/camel"));
        assert_eq!(
            "org/apache/camel/",
            m.determine_root_dir("org/apache/camel/")
        );
        assert_eq!(
            "org/apache/camel/",
            m.determine_root_dir("org/apache/camel/*.xml")
        );
        assert_eq!("WEB-INF/", m.determine_root_dir("WEB-INF/*.xml"));
        // this is not a pattern
        assert_eq!(
            "org/apache/camel/mycamel.xml",
            m.determine_root_dir("org/apache/camel/mycamel.xml")
        );
    }

    #[test]
    fn test_static_instance() {
        assert!(INSTANCE.match_("*.txt", "blah.txt"));
    }
}
