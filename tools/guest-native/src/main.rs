include!(concat!(env!("OUT_DIR"), "/methods.rs"));

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "lez_registry.bin".into());
    std::fs::write(&out, LEZ_REGISTRY_ELF).expect("write the program binary");
    let id: Vec<u8> = LEZ_REGISTRY_ID.iter().flat_map(|w| w.to_le_bytes()).collect();
    let hex: String = id.iter().map(|b| format!("{b:02x}")).collect();
    println!("{out}\nImageID {hex}");
}
