//! Test-only JSON-lines driver. Ceremony state stays in this native process.
use openrust_passkeys::{AuthenticationOptions, RegistrationOptions, StoredCredential, WebAuthn};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{self, BufRead, Write},
};

#[derive(Default)]
struct Driver {
    web: Option<WebAuthn>,
    registrations: HashMap<String, RegistrationOptions>,
    verifications: HashMap<String, AuthenticationOptions>,
    credentials: HashMap<String, StoredCredential>,
}
impl Driver {
    fn call(&mut self, value: Value) -> Result<Value, Box<dyn std::error::Error>> {
        let op = value["op"].as_str().ok_or("missing op")?;
        if op == "init" {
            self.web = Some(WebAuthn::new(
                "localhost",
                &[value["origin"].as_str().ok_or("origin")?.into()],
                60000,
            )?);
            return Ok(json!(true));
        }
        let web = self.web.as_ref().ok_or("uninitialized")?;
        let key = value["key"].as_str().unwrap_or("default").to_owned();
        match op {
            "registration" => {
                let excluded = if value["exclude"] == true {
                    self.credentials.values().cloned().collect::<Vec<_>>()
                } else {
                    vec![]
                };
                let state = web.registration_options(
                    value["handle"].as_str().unwrap_or(&"a".repeat(64)),
                    "email@example.com",
                    "Name",
                    &excluded,
                )?;
                let result = state.options().clone();
                self.registrations.insert(key, state);
                Ok(result)
            }
            "register" => {
                let state = self.registrations.remove(&key).ok_or("consumed")?;
                let credential = web.register(value["credential"].clone(), &state)?;
                let id = credential.credential_id();
                self.credentials.insert(id.clone(), credential);
                Ok(json!(id))
            }
            "stored" => {
                let credential = self
                    .credentials
                    .get_mut(value["id"].as_str().ok_or("id")?)
                    .ok_or("missing credential")?;
                let stored = serde_json::to_value(&*credential)?;
                if value["legacy"] == true {
                    let mut legacy = stored.clone();
                    legacy
                        .as_object_mut()
                        .ok_or("stored object")?
                        .remove("aaguid");
                    *credential = serde_json::from_value(legacy)?;
                    return Ok(serde_json::to_value(&*credential)?);
                }
                Ok(stored)
            }
            "verification" => {
                let allowed = if value["allow"] == true {
                    self.credentials.values().cloned().collect::<Vec<_>>()
                } else {
                    vec![]
                };
                let state = web.verification_options(&allowed)?;
                let result = state.options().clone();
                self.verifications.insert(key, state);
                Ok(result)
            }
            "verify" => {
                let state = self.verifications.remove(&key).ok_or("consumed")?;
                let id = value["credential"]["rawId"].as_str().ok_or("rawId")?;
                let credential = self.credentials.get_mut(id).ok_or("missing credential")?;
                web.verify(
                    value["credential"].clone(),
                    &state,
                    credential,
                    value["discoverable"] != false,
                )?;
                Ok(json!({"counter":credential.counter(),"handle":credential.user_handle()}))
            }
            _ => Err("unknown op".into()),
        }
    }
}
fn main() {
    if !std::env::args().any(|a| a == "--protocol") {
        return;
    }
    let mut driver = Driver::default();
    for line in io::stdin().lock().lines() {
        let result = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .and_then(|v| driver.call(v).map_err(|e| e.to_string()));
        let output = match result {
            Ok(value) => json!({"ok":value}),
            Err(error) => json!({"error":error}),
        };
        println!("{output}");
        io::stdout().flush().unwrap();
    }
}
