use keyring::Entry;

use crate::settings::Settings;

const SERVICE: &str = "com.claire.desktop";

const ACCOUNTS: &[(
    &str,
    fn(&Settings) -> &str,
    fn(&mut Settings) -> &mut String,
)] = &[
    (
        "openai",
        |settings| settings.openai_api_key.as_str(),
        |settings| &mut settings.openai_api_key,
    ),
    (
        "anthropic",
        |settings| settings.anthropic_api_key.as_str(),
        |settings| &mut settings.anthropic_api_key,
    ),
    (
        "custom",
        |settings| settings.custom_api_key.as_str(),
        |settings| &mut settings.custom_api_key,
    ),
    (
        "tavily",
        |settings| settings.tavily_api_key.as_str(),
        |settings| &mut settings.tavily_api_key,
    ),
    (
        "brave",
        |settings| settings.brave_api_key.as_str(),
        |settings| &mut settings.brave_api_key,
    ),
];

trait SecretStore {
    fn read(&self, account: &str) -> Result<Option<String>, String>;
    fn write(&self, account: &str, secret: &str) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

struct KeyringStore;

impl SecretStore for KeyringStore {
    fn read(&self, account: &str) -> Result<Option<String>, String> {
        let entry = Entry::new(SERVICE, account)
            .map_err(|err| format!("credential store ({account}): {err}"))?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(format!(
                "Could not read the {account} API key from the system credential store: {err}"
            )),
        }
    }

    fn write(&self, account: &str, secret: &str) -> Result<(), String> {
        let entry = Entry::new(SERVICE, account)
            .map_err(|err| format!("credential store ({account}): {err}"))?;
        entry.set_password(secret).map_err(|err| {
            format!("Could not store the {account} API key in the system credential store: {err}")
        })
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        let entry = Entry::new(SERVICE, account)
            .map_err(|err| format!("credential store ({account}): {err}"))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(format!(
                "Could not remove the {account} API key from the system credential store: {err}"
            )),
        }
    }
}

/// True when `settings.json` still contains a raw credential or a usage map keyed by one.
pub fn plaintext_present(settings: &Settings) -> bool {
    if ACCOUNTS
        .iter()
        .any(|(_, get, _)| !get(settings).trim().is_empty())
    {
        return true;
    }
    settings
        .search_usage
        .tavily
        .keys()
        .chain(settings.search_usage.brave.keys())
        .any(|key| !key.is_empty() && !key.starts_with("sha256:"))
}

/// Load credentials from the OS store. A non-empty value still sitting in `settings` is migrated
/// into the store when the store has no entry yet. The store wins if both have a value.
pub fn hydrate(settings: &mut Settings) -> Result<(), String> {
    hydrate_with(settings, &KeyringStore)
}

fn hydrate_with(settings: &mut Settings, store: &impl SecretStore) -> Result<(), String> {
    for (account, get, get_mut) in ACCOUNTS {
        let from_file = get(settings).trim().to_string();
        match store.read(account)? {
            Some(stored) if !stored.is_empty() => {
                *get_mut(settings) = stored;
            }
            _ if !from_file.is_empty() => {
                store.write(account, &from_file)?;
                *get_mut(settings) = from_file;
            }
            _ => {
                *get_mut(settings) = String::new();
            }
        }
    }
    Ok(())
}

/// Write current credentials to the OS store. Empty fields remove the stored entry.
/// Unchanged values are left alone so search-count saves do not rewrite the keychain.
pub fn store(settings: &Settings) -> Result<(), String> {
    store_with(settings, &KeyringStore)
}

fn store_with(settings: &Settings, store: &impl SecretStore) -> Result<(), String> {
    for (account, get, _) in ACCOUNTS {
        let next = get(settings).trim();
        match store.read(account)? {
            Some(current) if current == next => {}
            Some(_) if next.is_empty() => store.delete(account)?,
            None if next.is_empty() => {}
            _ => store.write(account, next)?,
        }
    }
    Ok(())
}

pub fn redacted_json(settings: &Settings) -> Result<String, String> {
    let mut value = serde_json::to_value(settings).map_err(|err| err.to_string())?;
    let Some(object) = value.as_object_mut() else {
        return Err("settings did not serialize to an object".into());
    };
    for key in [
        "openaiApiKey",
        "anthropicApiKey",
        "customApiKey",
        "tavilyApiKey",
        "braveApiKey",
    ] {
        object.insert(key.to_string(), serde_json::Value::String(String::new()));
    }
    serde_json::to_string_pretty(&value).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    struct MemoryStore {
        values: Mutex<HashMap<String, String>>,
    }

    impl SecretStore for MemoryStore {
        fn read(&self, account: &str) -> Result<Option<String>, String> {
            Ok(self.values.lock().unwrap().get(account).cloned())
        }

        fn write(&self, account: &str, secret: &str) -> Result<(), String> {
            self.values
                .lock()
                .unwrap()
                .insert(account.to_string(), secret.to_string());
            Ok(())
        }

        fn delete(&self, account: &str) -> Result<(), String> {
            self.values.lock().unwrap().remove(account);
            Ok(())
        }
    }

    #[test]
    fn migrates_file_key_into_the_credential_store_and_strips_json() {
        let store = MemoryStore {
            values: Mutex::new(HashMap::new()),
        };
        let mut settings = Settings {
            openai_api_key: "sk-from-disk".into(),
            tavily_api_key: "tvly-from-disk".into(),
            ..Settings::default()
        };
        settings.search_usage.tavily.insert(
            "tvly-from-disk".into(),
            crate::settings::KeyUsage {
                month: "2026-09".into(),
                count: 3,
                monthly_limit: 10,
            },
        );
        assert!(plaintext_present(&settings));
        hydrate_with(&mut settings, &store).expect("hydrate");
        assert_eq!(settings.openai_api_key, "sk-from-disk");
        assert_eq!(
            store.read("openai").unwrap().as_deref(),
            Some("sk-from-disk")
        );
        settings.adopt_legacy_search_usage();
        let raw = redacted_json(&settings).unwrap();
        assert!(!raw.contains("sk-from-disk"));
        assert!(!raw.contains("tvly-from-disk"));
        assert!(raw.contains("sha256:"));

        let mut loaded: Settings = serde_json::from_str(&raw).unwrap();
        assert!(loaded.openai_api_key.is_empty());
        assert!(!plaintext_present(&loaded));
        hydrate_with(&mut loaded, &store).unwrap();
        assert_eq!(loaded.openai_api_key, "sk-from-disk");
        assert_eq!(loaded.tavily_api_key, "tvly-from-disk");

        loaded.openai_api_key.clear();
        store_with(&loaded, &store).unwrap();
        assert!(store.read("openai").unwrap().is_none());
    }
}
