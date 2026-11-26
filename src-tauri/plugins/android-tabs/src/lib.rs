use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

mod error;
#[cfg(mobile)]
mod mobile;
mod models;

pub use error::{Error, Result};
pub use models::{AndroidTabConfig, HasTabReply, LayoutConfig, PluginReply};

pub trait AndroidTabsExt<R: Runtime> {
    fn android_tabs(&self) -> &AndroidTabs<R>;
}

impl<R: Runtime, T: Manager<R>> AndroidTabsExt<R> for T {
    fn android_tabs(&self) -> &AndroidTabs<R> {
        self.state::<AndroidTabs<R>>().inner()
    }
}

pub struct AndroidTabs<R: Runtime>(mobile::AndroidTabs<R>);

impl<R: Runtime> AndroidTabs<R> {
    pub fn create_tab(&self, config: AndroidTabConfig) -> Result<()> {
        self.0.call::<PluginReply>("createTab", config)?;
        Ok(())
    }

    pub fn activate_tab(&self, id: &str, is_start: bool) -> Result<()> {
        self.0
            .call::<PluginReply>("activateTab", models::ActivateTab { id, is_start })?;
        Ok(())
    }

    pub fn close_tab(&self, id: &str) -> Result<()> {
        self.0
            .call::<PluginReply>("closeTab", models::TabId { id })?;
        Ok(())
    }

    pub fn navigate(&self, id: &str, url: &str) -> Result<()> {
        self.0
            .call::<PluginReply>("navigate", models::NavigateTab { id, url })?;
        Ok(())
    }

    pub fn history_move(&self, id: &str, direction: &str) -> Result<()> {
        self.0
            .call::<PluginReply>("historyMove", models::HistoryMove { id, direction })?;
        Ok(())
    }

    pub fn reload(&self, id: &str) -> Result<()> {
        self.0.call::<PluginReply>("reload", models::TabId { id })?;
        Ok(())
    }

    pub fn set_zoom(&self, id: &str, zoom: f64) -> Result<()> {
        self.0
            .call::<PluginReply>("setZoom", models::SetZoom { id, zoom })?;
        Ok(())
    }

    pub fn eval(&self, id: &str, script: &str) -> Result<()> {
        self.0
            .call::<PluginReply>("evalScript", models::EvalScript { id, script })?;
        Ok(())
    }

    pub fn set_layout(&self, config: LayoutConfig) -> Result<()> {
        self.0.call::<PluginReply>("setLayout", config)?;
        Ok(())
    }

    pub fn set_overlay(&self, enabled: bool) -> Result<()> {
        self.0
            .call::<PluginReply>("setOverlay", models::Overlay { enabled })?;
        Ok(())
    }

    pub fn open_download(&self, path: &str) -> Result<()> {
        self.0
            .call::<PluginReply>("openDownload", models::PathArg { path })?;
        Ok(())
    }

    pub fn share_page(&self, url: &str, title: &str) -> Result<()> {
        self.0
            .call::<PluginReply>("sharePage", models::SharePage { url, title })?;
        Ok(())
    }

    pub fn set_tracking_protection(&self, enabled: bool) -> Result<()> {
        self.0.call::<PluginReply>(
            "setTrackingProtection",
            models::TrackingProtection { enabled },
        )?;
        Ok(())
    }

    pub fn has_tab(&self, id: &str) -> Result<bool> {
        Ok(self
            .0
            .call::<HasTabReply>("hasTab", models::TabId { id })?
            .present)
    }

    pub fn request_default_browser(&self) -> Result<()> {
        self.0
            .call::<PluginReply>("requestDefaultBrowser", serde_json::json!({}))?;
        Ok(())
    }

    pub fn consume_external_url(&self) -> Result<Option<String>> {
        Ok(self
            .0
            .call::<models::PendingUrlReply>("consumeExternalUrl", serde_json::json!({}))?
            .url)
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("android-tabs")
        .setup(|app, api| {
            let tabs = mobile::init(app, api)?;
            app.manage(AndroidTabs(tabs));
            Ok(())
        })
        .build()
}
