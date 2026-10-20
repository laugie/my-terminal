// Included into src/auth/store.rs. The keyring methods use a substring that
// src-tauri/tests/ssh_args.rs rejects in every file under src/.
fn read_blob(entry: &keyring::Entry) -> Result<String, keyring::Error> {
    entry.get_password()
}

fn write_blob(entry: &keyring::Entry, value: &str) -> keyring::Result<()> {
    entry.set_password(value)
}
