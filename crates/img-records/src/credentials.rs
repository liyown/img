use anyhow::Result;
const SERVICE: &str = "dev.img.desktop.storage";
pub fn set(key: &str, value: &[u8]) -> Result<()> {
    #[cfg(target_os = "macos")]
    security_framework::passwords::set_generic_password(SERVICE, key, value)
        .map_err(|_| anyhow::anyhow!("cannot save credential to the system keychain"))?;
    #[cfg(not(target_os = "macos"))]
    keyring::Entry::new(SERVICE, key)?.set_secret(value)?;
    Ok(())
}
pub fn get(key: &str) -> Result<Vec<u8>> {
    #[cfg(target_os = "macos")]
    return security_framework::passwords::get_generic_password(SERVICE, key)
        .map_err(|_| anyhow::anyhow!("cannot read credential from the system keychain"));
    #[cfg(not(target_os = "macos"))]
    Ok(keyring::Entry::new(SERVICE, key)?.get_secret()?)
}
pub fn remove(key: &str) {
    #[cfg(target_os = "macos")]
    let _ = security_framework::passwords::delete_generic_password(SERVICE, key);
    #[cfg(not(target_os = "macos"))]
    if let Ok(entry) = keyring::Entry::new(SERVICE, key) {
        let _ = entry.delete_credential();
    }
}
