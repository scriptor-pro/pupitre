// Essai jetable : secret-service 4 (blocking) contre gnome-keyring.
use secret_service::blocking::SecretService;
use secret_service::EncryptionType;
use std::collections::HashMap;
use std::time::Instant;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let t = Instant::now();
    let ss = match SecretService::connect(EncryptionType::Dh) {
        Ok(s) => s,
        Err(e) => {
            println!("connect: ERREUR après {:?} : {e} / {e:?}", t.elapsed());
            return;
        }
    };
    println!("connect: ok en {:?}", t.elapsed());
    if mode == "absent" { return; }
    let col = ss.get_default_collection().expect("collection par défaut");
    let t = Instant::now();
    println!("is_locked = {:?} en {:?}", col.is_locked(), t.elapsed());
    if mode == "verrou" { return; }
    let mut attrs: HashMap<&str, &str> = HashMap::new();
    attrs.insert("application", "pupitre-essai");
    attrs.insert("cle", "/tmp/cle-essai");
    let it = col.create_item("Pupitre essai", attrs.clone(), b"secret1", true, "text/plain").expect("create");
    println!("écriture: ok");
    let r = col.search_items(attrs.clone()).expect("search");
    println!("recherche par attributs: {} résultat(s)", r.len());
    for i in &r { println!("  secret = {:?}", String::from_utf8(i.get_secret().unwrap())); }
    let mut autre = attrs.clone(); autre.insert("cle", "/autre");
    println!("recherche autre clé: {} résultat(s)", col.search_items(autre).unwrap().len());
    it.delete().expect("delete");
    println!("suppression: ok, restant = {}", col.search_items(attrs).unwrap().len());
}
