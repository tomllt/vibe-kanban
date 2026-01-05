pub fn is_valid_branch_prefix(prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }

    if prefix.contains('/') {
        return false;
    }

    git2::Branch::name_is_valid(&format!("{prefix}/x")).unwrap_or_default()
}

pub fn is_valid_branch_name(name: &str) -> bool {
    if name.trim().is_empty() {
        return false;
    }
    git2::Branch::name_is_valid(name).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_prefixes() {
        assert!(is_valid_branch_prefix(""));
        assert!(is_valid_branch_prefix("vk"));
        assert!(is_valid_branch_prefix("feature"));
        assert!(is_valid_branch_prefix("hotfix-123"));
        assert!(is_valid_branch_prefix("foo.bar"));
        assert!(is_valid_branch_prefix("foo_bar"));
        assert!(is_valid_branch_prefix("FOO-Bar"));
    }

    #[test]
    fn test_invalid_prefixes() {
        assert!(!is_valid_branch_prefix("foo/bar"));
        assert!(!is_valid_branch_prefix("foo..bar"));
        assert!(!is_valid_branch_prefix("foo@{"));
        assert!(!is_valid_branch_prefix("foo.lock"));
        // Note: git2 allows trailing dots in some contexts, but we enforce stricter rules
        // for prefixes by checking the full branch name format
        assert!(!is_valid_branch_prefix("foo bar"));
        assert!(!is_valid_branch_prefix("foo?"));
        assert!(!is_valid_branch_prefix("foo*"));
        assert!(!is_valid_branch_prefix("foo~"));
        assert!(!is_valid_branch_prefix("foo^"));
        assert!(!is_valid_branch_prefix("foo:"));
        assert!(!is_valid_branch_prefix("foo["));
        assert!(!is_valid_branch_prefix("/foo"));
        assert!(!is_valid_branch_prefix("foo/"));
        assert!(!is_valid_branch_prefix(".foo"));
    }

    #[test]
    fn test_valid_branch_names() {
        assert!(is_valid_branch_name("main"));
        assert!(is_valid_branch_name("staging"));
        assert!(is_valid_branch_name("feature/foo"));
    }

    #[test]
    fn test_invalid_branch_names() {
        assert!(!is_valid_branch_name(""));
        assert!(!is_valid_branch_name(" "));
        assert!(!is_valid_branch_name("foo..bar"));
        assert!(!is_valid_branch_name("foo lock"));
    }
}
