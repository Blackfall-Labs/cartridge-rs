//! Pattern matching for IAM resources
//!
//! Supports wildcards:
//! - `*` - Matches any single path segment (e.g., `/users/*/profile`)
//! - `**` - Matches any number of path segments recursively (e.g., `/admin/**`)

/// Pattern matcher for resource paths
pub struct PatternMatcher;

impl PatternMatcher {
    /// Check if a resource path matches a pattern
    ///
    /// # Examples
    /// ```
    /// use cartridge::iam::PatternMatcher;
    ///
    /// assert!(PatternMatcher::matches("/users/*", "/users/alice"));
    /// assert!(PatternMatcher::matches("/admin/**", "/admin/users/bob"));
    /// assert!(!PatternMatcher::matches("/users/*", "/admin/alice"));
    /// ```
    pub fn matches(pattern: &str, path: &str) -> bool {
        // Normalize paths
        let pattern = Self::normalize(pattern);
        let path = Self::normalize(path);

        Self::matches_normalized(&pattern, &path)
    }

    /// Normalize a path (remove trailing slashes, handle empty segments)
    fn normalize(path: &str) -> String {
        let path = path.trim_start_matches('/').trim_end_matches('/');
        if path.is_empty() {
            return "/".to_string();
        }
        format!("/{}", path)
    }

    /// Match normalized paths
    fn matches_normalized(pattern: &str, path: &str) -> bool {
        // Exact match
        if pattern == path {
            return true;
        }

        // Check for wildcards
        if !pattern.contains('*') {
            return false;
        }

        let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
        let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        Self::match_parts(&pattern_parts, &path_parts, 0, 0)
    }

    /// Recursively match pattern parts against path parts
    fn match_parts(pattern: &[&str], path: &[&str], pat_idx: usize, path_idx: usize) -> bool {
        // Both exhausted - match
        if pat_idx >= pattern.len() && path_idx >= path.len() {
            return true;
        }

        // Pattern exhausted but path remains - no match
        if pat_idx >= pattern.len() {
            return false;
        }

        // Path exhausted but pattern remains - only matches if remaining pattern is all **
        if path_idx >= path.len() {
            return pattern[pat_idx..].iter().all(|&p| p == "**");
        }

        let pat_part = pattern[pat_idx];
        let path_part = path[path_idx];

        match pat_part {
            // ** matches zero or more segments
            "**" => {
                // Try matching with ** consuming 0, 1, 2, ... segments
                for skip in 0..=(path.len() - path_idx) {
                    if Self::match_parts(pattern, path, pat_idx + 1, path_idx + skip) {
                        return true;
                    }
                }
                false
            }
            // * matches exactly one segment
            "*" => Self::match_parts(pattern, path, pat_idx + 1, path_idx + 1),
            // Literal match
            _ => {
                if pat_part == path_part {
                    Self::match_parts(pattern, path, pat_idx + 1, path_idx + 1)
                } else {
                    false
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert!(PatternMatcher::matches("/users/alice", "/users/alice"));
        assert!(!PatternMatcher::matches("/users/alice", "/users/bob"));
    }

    #[test]
    fn test_single_wildcard() {
        assert!(PatternMatcher::matches("/users/*", "/users/alice"));
        assert!(PatternMatcher::matches("/users/*", "/users/bob"));
        assert!(!PatternMatcher::matches("/users/*", "/users/alice/profile"));
        assert!(!PatternMatcher::matches("/users/*", "/admin/alice"));
    }

    #[test]
    fn test_recursive_wildcard() {
        assert!(PatternMatcher::matches("/admin/**", "/admin/users"));
        assert!(PatternMatcher::matches("/admin/**", "/admin/users/bob"));
        assert!(PatternMatcher::matches(
            "/admin/**",
            "/admin/users/bob/profile"
        ));
        assert!(!PatternMatcher::matches("/admin/**", "/users/admin"));
    }

    #[test]
    fn test_mixed_wildcards() {
        assert!(PatternMatcher::matches(
            "/users/*/profile",
            "/users/alice/profile"
        ));
        assert!(!PatternMatcher::matches(
            "/users/*/profile",
            "/users/alice/settings"
        ));

        assert!(PatternMatcher::matches(
            "/users/**/settings",
            "/users/alice/settings"
        ));
        assert!(PatternMatcher::matches(
            "/users/**/settings",
            "/users/alice/profile/settings"
        ));
    }

    #[test]
    fn test_normalization() {
        assert!(PatternMatcher::matches("/users/alice", "users/alice"));
        assert!(PatternMatcher::matches("users/alice/", "/users/alice"));
        assert!(PatternMatcher::matches("/users/alice/", "users/alice"));
    }

    #[test]
    fn test_root_path() {
        assert!(PatternMatcher::matches("/", "/"));
        assert!(PatternMatcher::matches("/*", "/anything"));
        assert!(PatternMatcher::matches("/**", "/anything/nested"));
    }

    #[test]
    fn test_multiple_wildcards() {
        assert!(PatternMatcher::matches(
            "/*/files/*",
            "/users/files/doc.txt"
        ));
        assert!(PatternMatcher::matches(
            "/*/files/*",
            "/admin/files/data.json"
        ));
        assert!(!PatternMatcher::matches(
            "/*/files/*",
            "/users/data/doc.txt"
        ));
    }

    #[test]
    fn test_recursive_at_end() {
        assert!(PatternMatcher::matches("/public/**", "/public/file.txt"));
        assert!(PatternMatcher::matches(
            "/public/**",
            "/public/dir/file.txt"
        ));
        assert!(PatternMatcher::matches("/public/**", "/public/a/b/c/d"));
    }

    #[test]
    fn test_no_match_cases() {
        assert!(!PatternMatcher::matches("/users/*", "/admin/alice"));
        assert!(!PatternMatcher::matches("/users/alice", "/users/bob"));
        assert!(!PatternMatcher::matches("/admin/**", "/users/test"));
    }
}
