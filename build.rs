//! Build script: embed the application icon into the Windows executable so it
//! shows up in Explorer, the taskbar, and Alt-Tab. No-op on other platforms.

fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(err) = res.compile() {
            // Don't fail the whole build just because the resource compiler is
            // unavailable; the app still runs, only the .exe icon is missing.
            println!("cargo:warning=failed to embed exe icon: {err}");
        }
    }
}
