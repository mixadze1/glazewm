use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{BindingModeConfig, ContainerDto, TilingDirection, WmEvent};

pub const DEFAULT_IPC_PORT: u32 = 6123;

/// Optional per-user port override for recovering from a stale IPC
/// listener.
pub fn ipc_port() -> anyhow::Result<u16> {
  let home =
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
  let Some(home) = home else {
    return Ok(DEFAULT_IPC_PORT as u16);
  };
  let path = std::path::PathBuf::from(home).join(".glzr/glazewm/ipc-port");
  match std::fs::read_to_string(path) {
    Ok(value) => {
      let port: u16 = value.trim().parse()?;
      anyhow::ensure!(port != 0, "IPC port must be nonzero");
      Ok(port)
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
      Ok(DEFAULT_IPC_PORT as u16)
    }
    Err(error) => Err(error.into()),
  }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "messageType", rename_all = "snake_case")]
pub enum ServerMessage {
  ClientResponse(ClientResponseMessage),
  EventSubscription(EventSubscriptionMessage),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientResponseMessage {
  pub client_message: String,
  pub data: Option<ClientResponseData>,
  pub error: Option<String>,
  pub success: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ClientResponseData {
  AppMetadata(AppMetadataData),
  BindingModes(BindingModesData),
  Command(CommandData),
  EventSubscribe(EventSubscribeData),
  EventUnsubscribe,
  Focused(FocusedData),
  Monitors(MonitorsData),
  TilingDirection(TilingDirectionData),
  Windows(WindowsData),
  Workspaces(WorkspacesData),
  Paused(bool),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMetadataData {
  pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingModesData {
  pub binding_modes: Vec<BindingModeConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandData {
  pub subject_container_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventSubscribeData {
  pub subscription_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusedData {
  pub focused: ContainerDto,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorsData {
  pub monitors: Vec<ContainerDto>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TilingDirectionData {
  pub tiling_direction: TilingDirection,
  pub direction_container: ContainerDto,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsData {
  pub windows: Vec<ContainerDto>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacesData {
  pub workspaces: Vec<ContainerDto>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventSubscriptionMessage {
  pub data: Option<WmEvent>,
  pub error: Option<String>,
  pub subscription_id: Uuid,
  pub success: bool,
}
