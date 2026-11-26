use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidTabConfig {
    pub id: String,
    pub url: String,
    pub width: f64,
    pub height: f64,
    pub chrome_height: f64,
    pub chrome_position: String,
    pub zoom: f64,
    pub tracking_protection: bool,
    pub init_script: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutConfig {
    pub width: f64,
    pub height: f64,
    pub chrome_height: f64,
    pub chrome_position: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivateTab<'a> {
    pub id: &'a str,
    pub is_start: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TabId<'a> {
    pub id: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NavigateTab<'a> {
    pub id: &'a str,
    pub url: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryMove<'a> {
    pub id: &'a str,
    pub direction: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetZoom<'a> {
    pub id: &'a str,
    pub zoom: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EvalScript<'a> {
    pub id: &'a str,
    pub script: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Overlay {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrackingProtection {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PathArg<'a> {
    pub path: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SharePage<'a> {
    pub url: &'a str,
    pub title: &'a str,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginReply {
    pub ok: bool,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HasTabReply {
    pub present: bool,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUrlReply {
    pub url: Option<String>,
}
