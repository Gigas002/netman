// SPDX-License-Identifier: GPL-3.0-only

//! Mobile broadband enrichment via NM `Device.Modem` and ModemManager.

use std::collections::HashMap;

use tracing::warn;
use zbus::zvariant::OwnedValue;

use super::proxies::{
    DeviceModemProxy, DeviceProxy, MmModem3gppProxy, MmModemProxy, MmSimProxy, NetworkManagerProxy,
};
use crate::{
    Result,
    connection::{AccessTechnology, ModemInfo, ModemLiveData},
    error::Error,
};

/// NM device type constant for mobile broadband modems.
const DEVICE_TYPE_MODEM: u32 = 8;

/// ModemManager lock: SIM PIN required.
const MM_MODEM_LOCK_SIM_PIN: u32 = 2;

/// ModemManager access technology bit flags.
const MM_ACCESS_TECH_GSM: u32 = 0x2;
const MM_ACCESS_TECH_UMTS: u32 = 0x20;
const MM_ACCESS_TECH_LTE: u32 = 0x800;
const MM_ACCESS_TECH_5GNR: u32 = 0x10000;

/// NM modem capability bit flags.
const NM_MODEM_CAP_LTE: u32 = 0x4;
const NM_MODEM_CAP_5GNR: u32 = 0x8;

/// Build initial modem info from a saved GSM connection profile.
pub fn build_modem_info(raw: &HashMap<String, HashMap<String, OwnedValue>>) -> ModemInfo {
    let apn = raw
        .get("gsm")
        .and_then(|s| get_str_value(s, "apn"))
        .filter(|a| !a.is_empty());

    ModemInfo {
        apn,
        operator_name: None,
        operator_code: None,
        signal_quality: 0,
        access_technology: AccessTechnology::Unknown,
        sim_locked: false,
    }
}

/// Fetch live data from all modem devices managed by NetworkManager.
pub async fn fetch_modem_live_data(conn: &zbus::Connection) -> Vec<ModemLiveData> {
    let nm = match NetworkManagerProxy::new(conn).await {
        Ok(nm) => nm,
        Err(e) => {
            warn!(error = %e, "failed to reach NetworkManager for modem data");
            return Vec::new();
        }
    };

    let paths = match nm.devices().await {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "failed to list NM devices for modem data");
            return Vec::new();
        }
    };

    let mut results = Vec::new();
    for path in paths {
        match build_modem_live_data(conn, path.as_str()).await {
            Ok(Some(data)) => results.push(data),
            Ok(None) => {}
            Err(e) => warn!(?path, error = %e, "skipping modem device"),
        }
    }
    results
}

/// Send a SIM PIN to unlock the first locked modem.
pub async fn send_sim_pin(conn: &zbus::Connection, pin: &str) -> Result<()> {
    let nm = NetworkManagerProxy::new(conn).await?;
    for path in nm.devices().await? {
        let dev = DeviceProxy::builder(conn)
            .path(path.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;
        if dev.device_type().await? != DEVICE_TYPE_MODEM {
            continue;
        }

        let modem = DeviceModemProxy::builder(conn)
            .path(path.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;

        let device_id = modem.device_id().await.unwrap_or_default();
        if device_id.is_empty() {
            continue;
        }

        let mm = MmModemProxy::builder(conn)
            .path(device_id.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;

        let unlock = mm.unlock_required().await.unwrap_or(0);
        if unlock != MM_MODEM_LOCK_SIM_PIN {
            continue;
        }

        let sim_path = mm.sim().await?;
        if sim_path.as_str() == "/" {
            return Err(Error::OperationFailed("no SIM present".into()));
        }

        let sim = MmSimProxy::builder(conn)
            .path(sim_path.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await?;

        sim.send_pin(pin).await?;
        return Ok(());
    }

    Err(Error::DeviceNotFound("locked modem".into()))
}

async fn build_modem_live_data(
    conn: &zbus::Connection,
    path: &str,
) -> Result<Option<ModemLiveData>> {
    let dev = DeviceProxy::builder(conn)
        .path(path)
        .map_err(|e| Error::DBus(e.to_string()))?
        .build()
        .await?;

    if dev.device_type().await? != DEVICE_TYPE_MODEM {
        return Ok(None);
    }

    let interface = dev.interface().await.unwrap_or_default();

    let modem = DeviceModemProxy::builder(conn)
        .path(path)
        .map_err(|e| Error::DBus(e.to_string()))?
        .build()
        .await?;

    let operator_code = modem.operator_code().await.ok().filter(|c| !c.is_empty());
    let apn = modem.apn().await.ok().filter(|a| !a.is_empty());
    let current_caps = modem.current_capabilities().await.unwrap_or(0);

    let device_id = modem.device_id().await.unwrap_or_default();
    let mut signal_quality = 0u8;
    let mut access_technology = caps_to_access_technology(current_caps);
    let mut operator_name = None;
    let mut sim_locked = false;

    if !device_id.is_empty()
        && let Ok(mm) = MmModemProxy::builder(conn)
            .path(device_id.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await
    {
        if let Ok((quality, _recent)) = mm.signal_quality().await {
            signal_quality = quality.min(100) as u8;
        }
        let unlock = mm.unlock_required().await.unwrap_or(0);
        sim_locked = unlock == MM_MODEM_LOCK_SIM_PIN;

        if let Ok(tech) = mm.access_technologies().await {
            access_technology = mm_access_technology(tech);
        }

        if let Ok(mm3gpp) = MmModem3gppProxy::builder(conn)
            .path(device_id.as_str())
            .map_err(|e| Error::DBus(e.to_string()))?
            .build()
            .await
        {
            operator_name = mm3gpp.operator_name().await.ok().filter(|n| !n.is_empty());
        }
    }

    Ok(Some(ModemLiveData {
        interface,
        apn,
        operator_name,
        operator_code,
        signal_quality,
        access_technology,
        sim_locked,
    }))
}

fn caps_to_access_technology(caps: u32) -> AccessTechnology {
    if caps & NM_MODEM_CAP_5GNR != 0 {
        AccessTechnology::Nr5G
    } else if caps & NM_MODEM_CAP_LTE != 0 {
        AccessTechnology::Lte
    } else {
        AccessTechnology::Unknown
    }
}

fn mm_access_technology(tech: u32) -> AccessTechnology {
    if tech & MM_ACCESS_TECH_5GNR != 0 {
        AccessTechnology::Nr5G
    } else if tech & MM_ACCESS_TECH_LTE != 0 {
        AccessTechnology::Lte
    } else if tech & MM_ACCESS_TECH_UMTS != 0 {
        AccessTechnology::Umts
    } else if tech & MM_ACCESS_TECH_GSM != 0 {
        AccessTechnology::Gsm
    } else {
        AccessTechnology::Unknown
    }
}

fn get_str_value(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    let v = map.get(key)?;
    <&str>::try_from(v).ok().map(str::to_owned)
}

#[cfg(test)]
mod tests;
