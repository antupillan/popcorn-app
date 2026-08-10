use keyring::Entry;

/// Identificador de servicio para el keychain del SO — identidad de la app,
/// no config de usuario (comparable al `identifier` de tauri.conf.json), por
/// eso es la única constante literal permitida acá bajo el Mandato 5.
const SERVICE: &str = "popcorn-p2p";

fn entry(id: &str) -> anyhow::Result<Entry> {
    Ok(Entry::new(SERVICE, id)?)
}

/// Guarda un secreto de usuario (API key de un proveedor de IA, password de
/// un motor de torrents externo, etc.) en el keychain del SO, indexado por
/// un `id` propio del caller. Nunca en SQLite — ver Mandato de
/// Configurabilidad Soberana. Genérico a propósito: cualquier subsistema que
/// necesite guardar un secreto por id reusa este módulo en vez de duplicar
/// la lógica de `keyring::Entry`.
pub fn set_secret(id: &str, secret: &str) -> anyhow::Result<()> {
    entry(id)?.set_password(secret)?;
    Ok(())
}

pub fn get_secret(id: &str) -> anyhow::Result<Option<String>> {
    match entry(id)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_secret(id: &str) -> anyhow::Result<()> {
    match entry(id)?.delete_credential() {
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
        let id = "popcorn-test-secret";
        delete_secret(id).unwrap();
        assert_eq!(get_secret(id).unwrap(), None);

        set_secret(id, "sk-test-12345").unwrap();
        assert_eq!(get_secret(id).unwrap(), Some("sk-test-12345".to_string()));

        delete_secret(id).unwrap();
        assert_eq!(get_secret(id).unwrap(), None);
    }
}
