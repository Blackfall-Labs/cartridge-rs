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

    /// Normalize a path (remove trailing slashes, handle empty segments, resolve .. and .)
    fn normalize(path: &str) -> String {
        let path = path.trim_start_matches('/').trim_end_matches('/');
        if path.is_empty() {
            return "/".to_string();
        }

        // Resolve . and .. components
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut resolved: Vec<&str> = Vec::new();

        for part in parts {
            match part {
                "." => {
                    // Current directory, skip
                    continue;
                }
                ".." => {
                    // Parent directory, pop if possible
                    resolved.pop();
                }
                _ => {
                    resolved.push(part);
                }
            }
        }

        if resolved.is_empty() {
            return "/".to_string();
        }

        format!("/{}", resolved.join("/"))
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
            // Pattern with wildcards (e.g., *.txt, file-*)
            _ if pat_part.contains('*') => {
                if Self::match_glob_segment(pat_part, path_part) {
                    Self::match_parts(pattern, path, pat_idx + 1, path_idx + 1)
                } else {
                    false
                }
            }
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

    /// Match a glob pattern segment against a path segment
    /// Supports * within segments (e.g., *.txt, file-*, test-*-data)
    fn match_glob_segment(pattern: &str, segment: &str) -> bool {
        let parts: Vec<&str> = pattern.split('*').collect();

        // If pattern is just "*", it matches any segment
        if parts.len() == 2 && parts[0].is_empty() && parts[1].is_empty() {
            return true;
        }

        let mut pos = 0;

        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }

            // First part must match at beginning
            if i == 0 {
                if !segment.starts_with(part) {
                    return false;
                }
                pos = part.len();
            }
            // Last part must match at end
            else if i == parts.len() - 1 {
                if !segment.ends_with(part) {
                    return false;
                }
                // Check that end position is after current position
                if segment.len() < pos + part.len() {
                    return false;
                }
            }
            // Middle parts must exist in order
            else {
                if let Some(found_pos) = segment[pos..].find(part) {
                    pos += found_pos + part.len();
                } else {
                    return false;
                }
            }
        }

        true
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

    #[test]
    fn test_path_traversal_normalization() {
        // Path traversal should be normalized, preventing bypass attempts
        assert!(PatternMatcher::matches(
            "/private/**",
            "/public/../private/secret.txt"
        ));
        assert!(PatternMatcher::matches(
            "/private/**",
            "/public/./../private/secret.txt"
        ));

        // After normalization, /public/../private/file.txt becomes /private/file.txt
        assert!(!PatternMatcher::matches(
            "/public/**",
            "/public/../private/file.txt"
        ));

        // Current directory references should be normalized
        assert!(PatternMatcher::matches(
            "/users/alice",
            "/users/./alice"
        ));
    }

    #[test]
    fn test_glob_patterns_in_segments() {
        // *.txt pattern
        assert!(PatternMatcher::matches("/data/*.txt", "/data/file.txt"));
        assert!(PatternMatcher::matches("/data/*.txt", "/data/document.txt"));
        assert!(!PatternMatcher::matches("/data/*.txt", "/data/file.json"));
        assert!(!PatternMatcher::matches("/data/*.txt", "/data/subdir/file.txt"));

        // file-* pattern
        assert!(PatternMatcher::matches("/data/file-*", "/data/file-123"));
        assert!(PatternMatcher::matches("/data/file-*", "/data/file-abc"));
        assert!(!PatternMatcher::matches("/data/file-*", "/data/other-123"));

        // *-data pattern
        assert!(PatternMatcher::matches("/logs/*-data", "/logs/test-data"));
        assert!(PatternMatcher::matches("/logs/*-data", "/logs/prod-data"));
        assert!(!PatternMatcher::matches("/logs/*-data", "/logs/data-test"));

        // Multiple wildcards in segment
        assert!(PatternMatcher::matches(
            "/logs/*-*-*.log",
            "/logs/app-prod-2024.log"
        ));
    }
}
