//! Pure precedence rule for the existing OAuth encryption secret sources.
use zeroize::Zeroizing;

pub fn resolve_encryption_password(
    configured: Option<&str>,
    explicit_env: Option<String>,
    mounted_secret: Option<String>,
) -> Option<Zeroizing<String>> {
    explicit_env
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            configured
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
        })
        .or_else(|| mounted_secret.filter(|value| !value.trim().is_empty()))
        .map(Zeroizing::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_environment_overrides_yaml_and_mounted_secret() {
        let value = resolve_encryption_password(
            Some("yaml-key"),
            Some("explicit-key".into()),
            Some("mounted-key".into()),
        );
        assert_eq!(
            value.as_ref().map(|value| value.as_str()),
            Some("explicit-key")
        );
    }
    #[test]
    fn mounted_secret_works_and_empty_values_do_not_mask_it() {
        let value =
            resolve_encryption_password(Some(""), Some("  ".into()), Some("mounted-key".into()));
        assert_eq!(
            value.as_ref().map(|value| value.as_str()),
            Some("mounted-key")
        );
        assert!(resolve_encryption_password(None, None, None).is_none());
    }
    #[test]
    fn yaml_is_the_fallback_before_the_mounted_key() {
        let value = resolve_encryption_password(Some("yaml-key"), None, Some("mounted-key".into()));
        assert_eq!(value.as_ref().map(|value| value.as_str()), Some("yaml-key"));
    }
}
