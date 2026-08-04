use keyring::Entry;

/// Identificador de servicio para el keychain del SO — identidad de la app,
/// no config de usuario (comparable al `identifier` de tauri.conf.json), por
/// eso es la única constante literal permitida acá bajo el Mandato 5.
const SERVICE: &str = "popcorn-p2p";

fn entry(provider_id: &str) -> anyhow::Result<Entry> {
    Ok(Entry::new(SERVICE, provider_id)?)
}

/// Guarda la API key del usuario para un proveedor en el keychain del SO.
/// Nunca en SQLite — ver Mandato de Configurabilidad Soberana.
pub fn set_api_key(provider_id: &str, key: &str) -> anyhow::Result<()> {
    entry(provider_id)?.set_password(key)?;
    Ok(())
}

pub fn get_api_key(provider_id: &str) -> anyhow::Result<Option<String>> {
    match entry(provider_id)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_api_key(provider_id: &str) -> anyhow::Result<()> {
    match entry(provider_id)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifica contra el keychain real del SO (secret-service/D-Bus en este
    // entorno), no un mock — corre en #[ignore] porque un CI headless sin
    // sesión D-Bus con un secret-service activo no puede completarlo.
    #[test]
    #[ignore = "requiere keychain real del SO (secret-service/D-Bus, Keychain de macOS o Credential Manager de Windows)"]
    fn set_get_delete_roundtrip_against_real_os_keychain() {
        let provider_id = "popcorn-test-provider";
        delete_api_key(provider_id).unwrap();
        assert_eq!(get_api_key(provider_id).unwrap(), None);

        set_api_key(provider_id, "sk-test-12345").unwrap();
        assert_eq!(
            get_api_key(provider_id).unwrap(),
            Some("sk-test-12345".to_string())
        );

        delete_api_key(provider_id).unwrap();
        assert_eq!(get_api_key(provider_id).unwrap(), None);
    }
}
