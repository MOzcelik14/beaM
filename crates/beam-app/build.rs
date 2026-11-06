fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../../packaging/windows/beam.ico");
        res.set("FileDescription", "beaM. RAW Photo Editor");
        res.set("ProductName", "beaM.");
        res.set("LegalCopyright", "GPL-3.0-only");
        if let Err(e) = res.compile() {
            eprintln!("Warning: Failed to compile Windows resource: {e}");
        }
    }
}
