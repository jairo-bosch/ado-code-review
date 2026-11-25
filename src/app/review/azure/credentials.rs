use secrecy::{ExposeSecret, SecretString};

use super::AzureError;

pub(super) fn from_env() -> Result<SecretString, AzureError> {
    let token = std::env::var("AZURE_TOKEN").map_err(|_| AzureError::Token)?;
    validate(SecretString::from(token))
}

fn validate(token: SecretString) -> Result<SecretString, AzureError> {
    let value = token.expose_secret();
    if value.trim().is_empty() || value.contains("$(") || value.chars().any(char::is_control) {
        return Err(AzureError::Token);
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_debug_is_redacted() {
        let token = validate("sample".into()).unwrap();
        assert!(!format!("{token:?}").contains("sample"));
    }

    #[test]
    fn rejects_empty_tokens_and_unexpanded_macros() {
        for token in ["", " ", "$(Azure.Secret)", "sample\n"] {
            let error = validate(token.into()).unwrap_err();
            assert!(matches!(error, AzureError::Token));
            assert_eq!(
                error.to_string(),
                "missing or invalid environment variable: AZURE_TOKEN"
            );
        }
    }
}
