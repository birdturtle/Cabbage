//! Client requirements and consent-driven setup. Linux installers are a later phase.
use serde::Serialize;
use std::sync::{Arc, Mutex};
#[cfg(windows)]
pub mod windows;
const VC_MESSAGE: &str = "Microsoft Visual C++ Runtime is required for this client.";
#[derive(Default)]
pub struct State {
    pub busy: bool,
    pub message: String,
    pub vc_ready: bool,
}
pub type Shared = Arc<Mutex<State>>;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub required: bool,
    pub busy: bool,
    pub message: String,
}
pub fn status(state: &Shared, client: crate::clients::ClientId) -> Status {
    let state = state.lock().unwrap();
    let required =
        cfg!(windows) && client == crate::clients::ClientId::OfficialOsrs && !state.vc_ready;
    Status {
        required,
        busy: state.busy,
        message: if state.busy
            || client == crate::clients::ClientId::OfficialOsrs && !state.message.is_empty()
        {
            state.message.clone()
        } else if required {
            VC_MESSAGE.into()
        } else {
            String::new()
        },
    }
}
pub fn initial() -> Shared {
    let state = State {
        vc_ready: {
            #[cfg(windows)]
            {
                windows::vc_ready()
            }
            #[cfg(not(windows))]
            {
                true
            }
        },
        ..Default::default()
    };
    Arc::new(Mutex::new(state))
}
#[derive(Clone, Copy)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Installer {
    WebView,
    VisualCpp,
}
#[cfg_attr(not(windows), allow(dead_code))]
impl Installer {
    pub fn url(self) -> &'static str {
        match self {
            Self::WebView => "https://go.microsoft.com/fwlink/p/?LinkId=2124703",
            Self::VisualCpp => "https://aka.ms/vc14/vc_redist.x64.exe",
        }
    }
    pub fn arguments(self) -> &'static str {
        match self {
            Self::WebView => "/install",
            Self::VisualCpp => "/install /passive /norestart",
        }
    }
}
#[cfg_attr(not(windows), allow(dead_code))]
fn trusted_url(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.host_str().is_some_and(|host| {
            matches!(host, "aka.ms" | "microsoft.com") || host.ends_with(".microsoft.com")
        })
}
#[cfg_attr(not(windows), allow(dead_code))]
fn installer_result(code: u32) -> Result<bool, String> {
    match code {
        0 => Ok(false),
        3010 | 1641 => Ok(true),
        1602 => Err("Installation cancelled. You can retry when ready.".into()),
        _ => Err(format!(
            "Installation failed (code {code}). Retry or install the runtime from Microsoft."
        )),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_microsoft_https_downloads_are_accepted() {
        for installer in [Installer::WebView, Installer::VisualCpp] {
            assert!(trusted_url(&url::Url::parse(installer.url()).unwrap()));
        }
        for url in [
            "https://download.microsoft.com/runtime.exe",
            "https://msedge.sf.dl.delivery.mp.microsoft.com/runtime.exe",
        ] {
            assert!(trusted_url(&url::Url::parse(url).unwrap()));
        }
        for url in [
            "http://download.microsoft.com/runtime.exe",
            "https://microsoft.com.example.com/runtime.exe",
            "https://example.com/runtime.exe",
            "https://user@microsoft.com/runtime.exe",
            "https://microsoft.com:444/runtime.exe",
        ] {
            assert!(!trusted_url(&url::Url::parse(url).unwrap()));
        }
    }
    #[test]
    fn installer_exit_codes_distinguish_cancel_failure_and_restart() {
        assert_eq!(installer_result(0).unwrap(), false);
        assert_eq!(installer_result(3010).unwrap(), true);
        assert!(installer_result(1602).unwrap_err().contains("cancelled"));
        assert!(installer_result(1).is_err());
    }
}
