use super::{logs, mangle_unit, ReadAction, MGR_IFACE, MGR_PATH, PROPS_IFACE, UNIT_IFACE};
use crate::capabilities::View;
use crate::cli::Cli;
use crate::error::{FezError, Result};
use crate::protocol::client::BridgeClient;
use crate::protocol::variant::Variant;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceUnit {
    name: String,
    description: String,
    load_state: String,
    active_state: String,
    sub_state: String,
}

/// systemd `Unit` interface properties read via `Properties.GetAll`.
///
/// cockpit delivers this `a{sv}` dict with each value wrapped as a D-Bus
/// variant (`{"t":"s","v":"active"}`); [`Variant`] unwraps it transparently.
/// Every field is `#[serde(default)]` so an absent property decodes to the
/// empty string, matching the previous `s()` accessor's tolerance.
#[derive(Debug, Default, Deserialize)]
struct UnitProps {
    #[serde(rename = "Id", default)]
    id: Variant<String>,
    #[serde(rename = "Description", default)]
    description: Variant<String>,
    #[serde(rename = "LoadState", default)]
    load_state: Variant<String>,
    #[serde(rename = "ActiveState", default)]
    active_state: Variant<String>,
    #[serde(rename = "SubState", default)]
    sub_state: Variant<String>,
    #[serde(rename = "UnitFileState", default)]
    unit_file_state: Variant<String>,
}

pub(super) fn run(cli: &Cli, action: ReadAction<'_>) -> Result<View> {
    let mut client = crate::capabilities::connect(cli)?;
    let host = client.host().to_string();
    match action {
        ReadAction::List { state } => list(&mut client, host, state),
        ReadAction::Status { unit } => status(&mut client, host, &mangle_unit(unit)),
        ReadAction::Logs {
            unit,
            since,
            priority,
            lines,
            follow,
        } => logs::run(
            &mut client,
            host,
            cli.json,
            &mangle_unit(unit),
            since,
            priority,
            lines,
            follow,
        ),
    }
}

fn protocol_decode_error(message: impl Into<String>) -> FezError {
    FezError::Decode(<serde_json::Error as serde::de::Error>::custom(
        message.into(),
    ))
}

fn required_row_string(row: &Value, row_index: usize, idx: usize, field: &str) -> Result<String> {
    row.get(idx)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            protocol_decode_error(format!(
                "ListUnits row {row_index} missing string field {idx} ({field})"
            ))
        })
}

fn parse_service_unit(row: &Value, row_index: usize) -> Result<ServiceUnit> {
    Ok(ServiceUnit {
        name: required_row_string(row, row_index, 0, "name")?,
        description: required_row_string(row, row_index, 1, "description")?,
        load_state: required_row_string(row, row_index, 2, "load_state")?,
        active_state: required_row_string(row, row_index, 3, "active_state")?,
        sub_state: required_row_string(row, row_index, 4, "sub_state")?,
    })
}

fn parse_list_units(out: &Value) -> Result<Vec<ServiceUnit>> {
    let units = out
        .get(0)
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_decode_error("ListUnits reply missing unit array"))?;

    units
        .iter()
        .enumerate()
        .map(|(idx, row)| parse_service_unit(row, idx))
        .collect()
}

fn get_unit_path(out: &Value, unit: &str) -> Result<String> {
    out.get(0)
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            protocol_decode_error(format!("GetUnit reply for {unit} missing object path"))
        })
}

fn list(client: &mut BridgeClient, host: String, state: Option<&str>) -> Result<View> {
    let channel = client.dbus_open("org.freedesktop.systemd1")?;
    let out = client.dbus_call(&channel, MGR_PATH, MGR_IFACE, "ListUnits", json!([]))?;
    let units = parse_list_units(&out)?;

    let filtered: Vec<ServiceUnit> = if let Some(want) = state {
        units
            .into_iter()
            .filter(|u| u.active_state == want)
            .collect()
    } else {
        units
    };

    Ok(build_service_list(&filtered, host))
}

/// Render a filtered unit list into the columnar `ServiceList` view.
///
/// Columnar projection via the shared envelope helper: field names are stated
/// once in `columns`, and each unit becomes a positional row aligned to that
/// order. This keeps the `--json` payload compact for LLM consumers.
fn build_service_list(filtered: &[ServiceUnit], host: String) -> View {
    let mut human = format!(
        "{:<28} {:<10} {:<10} {}\n",
        "UNIT", "ACTIVE", "SUB", "DESCRIPTION"
    );
    for unit in filtered {
        human.push_str(&format!(
            "{:<28} {:<10} {:<10} {}\n",
            unit.name, unit.active_state, unit.sub_state, unit.description
        ));
    }
    let columns = [
        "name",
        "description",
        "load_state",
        "active_state",
        "sub_state",
    ];
    let rows: Vec<Value> = filtered
        .iter()
        .map(|unit| {
            json!([
                unit.name,
                unit.description,
                unit.load_state,
                unit.active_state,
                unit.sub_state,
            ])
        })
        .collect();
    View::new(
        "ServiceList",
        host,
        crate::envelope::table_data(&columns, rows),
        human,
    )
}

fn status(client: &mut BridgeClient, host: String, unit: &str) -> Result<View> {
    let channel = client.dbus_open("org.freedesktop.systemd1")?;
    let got = client.dbus_call(&channel, MGR_PATH, MGR_IFACE, "GetUnit", json!([unit]));
    let path = match got {
        Ok(out) => get_unit_path(&out, unit)?,
        Err(FezError::Dbus { name, .. }) if name.contains("NoSuchUnit") => {
            return Err(FezError::NotFound(unit.to_string()))
        }
        Err(e) => return Err(e),
    };
    let out = client.dbus_call(&channel, &path, PROPS_IFACE, "GetAll", json!([UNIT_IFACE]))?;
    let props_val = out.get(0).cloned().unwrap_or_else(|| json!({}));
    let props: UnitProps = serde_json::from_value(props_val).map_err(FezError::Decode)?;

    let data = json!({
        "id": props.id.0,
        "description": props.description.0,
        "load_state": props.load_state.0,
        "active_state": props.active_state.0,
        "sub_state": props.sub_state.0,
        "unit_file_state": props.unit_file_state.0,
    });
    let human = format!(
        "{} - {}\n  state: {} ({})\n  enabled: {}\n",
        props.id.0,
        props.description.0,
        props.active_state.0,
        props.sub_state.0,
        props.unit_file_state.0
    );
    Ok(View::new("ServiceStatus", host, data, human))
}

#[cfg(test)]
mod tests {
    use super::{get_unit_path, parse_list_units, UnitProps};
    use crate::error::FezError;
    use serde_json::json;

    // Real cockpit-bridge returns a{sv} dicts with each value wrapped as a
    // D-Bus variant: {"t":"s","v":"active"}. UnitProps unwraps via Variant<T>.
    #[test]
    fn unit_props_unwraps_variant_dict() {
        let props: UnitProps = serde_json::from_value(json!({
            "Id": {"t": "s", "v": "sshd.service"},
            "ActiveState": {"t": "s", "v": "active"},
            "UnitFileState": {"t": "s", "v": "enabled"},
        }))
        .unwrap();
        assert_eq!(props.id.0, "sshd.service");
        assert_eq!(props.active_state.0, "active");
        assert_eq!(props.unit_file_state.0, "enabled");
        // Absent properties default to the empty string, as `s()` did.
        assert_eq!(props.description.0, "");
    }

    #[test]
    fn parse_list_units_errors_when_unit_array_missing() {
        let err = parse_list_units(&json!([])).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn parse_list_units_errors_when_required_row_field_missing() {
        let err = parse_list_units(&json!([[["sshd.service", "OpenSSH server"]]])).unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn parse_list_units_errors_when_required_row_field_is_wrong_type() {
        let err = parse_list_units(&json!([[[
            "sshd.service",
            "OpenSSH server",
            "loaded",
            true,
            "running"
        ]]]))
        .unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }

    #[test]
    fn get_unit_path_errors_when_success_reply_has_no_object_path() {
        let err = get_unit_path(&json!([]), "sshd.service").unwrap_err();
        assert!(matches!(err, FezError::Decode(_)));
    }
}
