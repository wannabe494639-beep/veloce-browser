use serde::{de::DeserializeOwned, Serialize};
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<AndroidTabs<R>> {
    #[cfg(target_os = "android")]
    let handle =
        api.register_android_plugin("com.veloce.browser.androidtabs", "AndroidTabsPlugin")?;
    Ok(AndroidTabs(handle))
}

pub struct AndroidTabs<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> AndroidTabs<R> {
    pub fn call<T: DeserializeOwned>(
        &self,
        command: &str,
        payload: impl Serialize,
    ) -> crate::Result<T> {
        self.0
            .run_mobile_plugin(command, payload)
            .map_err(Into::into)
    }
}
