//! The session bus, as the desktop's own remotes are reached through it:
//! whether GVfs's daemon or kio-fuse is there or would be started when
//! asked, and kio-fuse's calls. Linux only. Each call connects again: a
//! few are made, never per file. Blocks: on a worker.

use std::ffi::OsString;
use std::time::Duration;

/// The bus the variables `env` name (a test's), else the session's; a
/// call on it waits at most `patience` for its answer.
fn connect(
    env: &[(OsString, OsString)],
    patience: Duration,
) -> anyhow::Result<zbus::blocking::Connection> {
    let named = env
        .iter()
        .find(|(key, _)| key == "DBUS_SESSION_BUS_ADDRESS")
        .and_then(|(_, value)| value.to_str());
    let builder = match named {
        Some(address) => zbus::blocking::connection::Builder::address(address)?,
        None => zbus::blocking::connection::Builder::session()?,
    };
    Ok(builder.method_timeout(patience).build()?)
}

/// Whether `name` answers on the bus, or the bus would start it when
/// asked (a service file names it): what a desktop installs, running or
/// not yet.
pub fn can_start(name: &str, env: &[(OsString, OsString)]) -> bool {
    let answers = || -> anyhow::Result<bool> {
        let bus = connect(env, Duration::from_secs(5))?;
        let proxy = zbus::blocking::fdo::DBusProxy::new(&bus)?;
        let name = zbus::names::BusName::try_from(name)?;
        if proxy.name_has_owner(name.clone())? {
            return Ok(true);
        }
        Ok(proxy
            .list_activatable_names()?
            .iter()
            .any(|n| n.as_str() == name.as_str()))
    };
    answers().unwrap_or(false)
}

/// Call `method` of `interface` on `destination`'s `path` with `body`,
/// waiting at most `patience` for its answer (a password asked in the
/// desktop's own dialog meanwhile).
///
/// # Errors
/// The bus's or the service's error, its words.
pub fn call<B, R>(
    env: &[(OsString, OsString)],
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
    patience: Duration,
) -> anyhow::Result<R>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
{
    let bus = connect(env, patience)?;
    let proxy = zbus::blocking::Proxy::new(&bus, destination, path, interface)?;
    match proxy.call(method, body) {
        Ok(answer) => Ok(answer),
        Err(zbus::Error::MethodError(_, Some(said), _)) => Err(anyhow::anyhow!("{said}")),
        Err(error) => Err(error.into()),
    }
}
