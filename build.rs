fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=assets/icons/geminidesk.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/geminidesk.ico");
        res.set("ProductName", "GeminiDesk");
        res.set(
            "FileDescription",
            "GeminiDesk — Desktop Client for Google Gemini",
        );
        res.set("CompanyName", "GeminiDesk Contributors");
        res.set("LegalCopyright", "MIT OR Apache-2.0");
        res.set("InternalName", "gemini-desk");
        res.set("OriginalFilename", "gemini-desk.exe");
        if let Err(err) = res.compile() {
            println!("cargo:warning=Failed to embed Windows icon/version resource: {err}");
        }
    }
}
