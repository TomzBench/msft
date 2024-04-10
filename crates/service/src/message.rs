//! Wrappers around windows_sys Service Control Message.  The Service Control Message is a message
//! from the kernel that is passed to system services. For additional details see:
//! https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nc-winsvc-lphandler_function_ex
use crate::util::{guid::Guid, sealed::Sealed, wchar};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use std::{
    error,
    ffi::{c_void, OsString},
    fmt,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use crossbeam::queue::SegQueue;
use futures::Stream;
use parking_lot::Mutex;
use tracing::{debug, error, warn};
use windows_sys::Win32::{
    Foundation::NO_ERROR,
    System::{Power::*, RemoteDesktop::*, Services::*, SystemServices::*},
    UI::WindowsAndMessaging::*,
};

pub trait TryCast: Sealed {
    unsafe fn try_cast(data: *mut c_void) -> Option<Self>
    where
        Self: Sized;
}

/// The ServiceMessageEx type is passed to the LPHANDLER_FUNCTION_EX that was registered via
/// RegisterServiceCtrlHandlerEx method. Note that this message will hold a reference to relevant
/// bytes. This is because some messages are ?Sized and we do not need to unecessarily allocate.
/// For additional details about each of the service messages, see:
/// https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nc-winsvc-lphandler_function_ex
pub enum ServiceMessageEx /*<D>*/ {
    ///  Notifies a paused service that it should resume.
    Continue,
    /// The handler should report its current status information to the SCM. IE: return NO_ERROR
    Interrogate,
    /// Notify a network service that there is a new component for binding. Applications should
    /// used plug and play functionality instead
    NetbindAdd,
    /// Notify a network service that one of its bindings has been disabled. The service should
    /// re-read its gbinding information and remove the binding. Applications should use plug and
    /// play functionality instead
    NetbindDisable,
    /// Notifies a network service that a disabled binding has been enabled. Applcations should use
    /// plug and play functionality instead
    NetbindEnable,
    /// Notifies a network service that a component for binding has been removed. Applications
    /// should use plug and play functionality instead
    NetbindRemove,
    /// Notifies a service that service specific startup parameters have changed. the service
    /// should reread its startup parameters
    ParamChange,
    /// Notifies a service that it should pause.
    Pause,
    /// Notifies a service that the system will be shutting down.
    Preshutdown,
    /// Notifies a service that the system is shutting down.
    Shutdown,
    /// Notifies a service that it should stop
    Stop,

    // The following events are Ex
    /// Notifies a service of device events. The service must have registered to receive these
    /// notifications using the
    /// [`windows_sys::Win32::UI::WindowsAndMessaging::RegisterDeviceNotificationW`] API.
    DeviceEvent(DeviceEvent),
    /// The computer's hardware profile has changed
    HardwareProfileChange(HardwareProfileChange),
    /// System power events, IE: battery changes state, etc
    PowerEvent(PowerSettingChange),
    /// User session change, IE: Login/Logout, etc. See [`SessionChange`]
    SessionChange(SessionChangeType, WTSSESSION_NOTIFICATION),
    /// The system time has changed
    TimeChange(SERVICE_TIMECHANGE_INFO),
    /// [See](https://learn.microsoft.com/en-us/windows/win32/services/service-trigger-events)
    TriggerEvent,
    /// Custom defined user event
    UserDefined(u8, u32, usize),
}

impl fmt::Display for ServiceMessageEx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Continue => write!(f, "continue"),
            Self::Interrogate => write!(f, "interrogate"),
            Self::NetbindAdd => write!(f, "netbind add"),
            Self::NetbindDisable => write!(f, "netbind disable"),
            Self::NetbindEnable => write!(f, "netbind enable"),
            Self::NetbindRemove => write!(f, "netbind remove"),
            Self::ParamChange => write!(f, "param change"),
            Self::Pause => write!(f, "pause"),
            Self::Preshutdown => write!(f, "pre-shutdown"),
            Self::Shutdown => write!(f, "shutdown"),
            Self::Stop => write!(f, "stop"),
            Self::DeviceEvent(e) => write!(f, "device event type => {e}"),
            Self::HardwareProfileChange(p) => write!(f, "hardware profile change => {p}"),
            Self::PowerEvent(ev) => write!(f, "power event => {ev}"),
            Self::SessionChange(s, _) => write!(f, "session change => {s} [[TODO]]"),
            Self::TimeChange(_) => write!(f, "time change => [[TODO]]"),
            Self::TriggerEvent => write!(f, "trigger event"),
            Self::UserDefined(c, e, _) => write!(f, "user defined => {c} {e}"),
        }
    }
}

impl fmt::Debug for ServiceMessageEx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

/// The event_type param of the Service Control Message when the Service Control Message is of a DeviceEvent
#[derive(FromPrimitive, Debug)]
#[repr(u32)]
pub enum DeviceEventType {
    Arrival = DBT_DEVICEARRIVAL,
    RemoveComplete = DBT_DEVICEREMOVECOMPLETE,
    QueryRemove = DBT_DEVICEQUERYREMOVE,
    QueryRemoveComplete = DBT_DEVICEQUERYREMOVEFAILED,
    RemovePending = DBT_DEVICEREMOVEPENDING,
    CustomEvent = DBT_CUSTOMEVENT,
}

impl fmt::Display for DeviceEventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arrival => write!(f, "device arrival"),
            Self::RemoveComplete => write!(f, "device remove complete"),
            Self::QueryRemove => write!(f, "device query remove"),
            Self::QueryRemoveComplete => write!(f, "device query remove complete"),
            Self::RemovePending => write!(f, "device remove pending"),
            Self::CustomEvent => write!(f, "device custom event"),
        }
    }
}

/// The event_data param of the Service Control Message when the Service Control Message is of a DeviceEvent
///
/// Extra data provided to the DeviceEvent message. NOTE that most of these types are of ?Sized
/// type and so we store them as a reference
#[repr(C)]
pub enum DeviceEventData {
    /// Contains information about a class of devices
    Interface(),
    /// Contains information about a file system handle
    Handle(),
    /// Contains information about a OEM-defined device type
    Oem(DEV_BROADCAST_OEM),
    /// Contains information about a modem, serial, or parallel port
    Port(OsString),
    /// Contains information about a logical volume
    Volume(DEV_BROADCAST_VOLUME),
}

impl fmt::Display for DeviceEventData {
    // TODO we should write things like the "port" name etc.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interface() => write!(f, "interface => [[TODO]]"),
            Self::Handle() => write!(f, "handle => [[TODO]]"),
            Self::Oem(_) => write!(f, "oem => [[TODO]]"),
            Self::Port(_) => write!(f, "port => [[TODO]]"),
            Self::Volume(_) => write!(f, "volume => [[TODO]]"),
        }
    }
}

impl fmt::Debug for DeviceEventData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interface() => write!(f, "DeviceEventData::Interface(TODO)"),
            Self::Handle() => write!(f, "DeviceEventData::Interface(TODO)"),
            Self::Oem(_) => write!(f, "DeviceEventData::Interface(TODO)"),
            Self::Port(port) => write!(f, "DeviceEventData::Port({:?})", port),
            Self::Volume(_) => write!(f, "DeviceEventData::Interface(TODO)"),
        }
    }
}

impl TryCast for DeviceEventData {
    /// Safety: The data pointer MUST be a DEV_BROADCAST_HDR.
    unsafe fn try_cast(data: *mut c_void) -> Option<Self> {
        let broadcast = &mut *(data as *mut DEV_BROADCAST_HDR);
        match broadcast.dbch_devicetype {
            DBT_DEVTYP_HANDLE => None,
            DBT_DEVTYP_OEM => None,
            DBT_DEVTYP_VOLUME => None,
            DBT_DEVTYP_DEVICEINTERFACE => None,
            DBT_DEVTYP_PORT => {
                let port = &*(data as *const DEV_BROADCAST_PORT_W);
                Some(Self::Port(wchar::from_wide(port.dbcp_name.as_ptr())))
            }
            _ => None,
        }
    }
}

impl Sealed for DeviceEventData {}

pub struct DeviceEvent {
    pub ty: DeviceEventType,
    pub data: DeviceEventData,
}
impl DeviceEvent {
    /// Safety: Data must be a Option<DEV_BROADCAST_HDR>
    pub(crate) unsafe fn try_parse(event_type: u32, data: *mut c_void) -> Option<Self> {
        Some(DeviceEvent {
            ty: DeviceEventType::from_u32(event_type)?,
            data: DeviceEventData::try_cast(data)?,
        })
    }

    /// Consume the device event and return the
    pub fn filter_port_arrival(self) -> Result<OsString, DeviceEvent> {
        match self.ty {
            DeviceEventType::Arrival => match self.data {
                DeviceEventData::Port(port) => Ok(port),
                _ => Err(self),
            },
            _ => Err(self),
        }
    }
}

impl fmt::Display for DeviceEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.ty, self.data)
    }
}

/// The event_type param of the ServiceControlMessage when the ServiceControlMessage is of a
/// HardwareProfileChange event
#[derive(FromPrimitive, Debug)]
#[repr(u32)]
pub enum HardwareProfileChange {
    /// [See](https://learn.microsoft.com/en-us/windows/win32/devio/dbt-configchanged)
    ConfigChanged = DBT_CONFIGCHANGED,
    /// [See](https://learn.microsoft.com/en-us/windows/win32/devio/dbt-querychangeconfig)
    QueryChangeConfig = DBT_QUERYCHANGECONFIG,
    /// [See](https://learn.microsoft.com/en-us/windows/win32/devio/dbt-configchangecanceled)
    ConfigChangeCanceled = DBT_CONFIGCHANGECANCELED,
}

impl fmt::Display for HardwareProfileChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfigChanged => write!(f, "config changed"),
            Self::QueryChangeConfig => write!(f, "query change config"),
            Self::ConfigChangeCanceled => write!(f, "config change canceled"),
        }
    }
}

/// The event_type param of the Service Control Message when the Service Control Message is of a
/// PowerSettingChange event
#[derive(Debug)]
#[repr(u32)]
pub enum PowerSettingChange {
    /// Power status has changed
    /// [See](https://learn.microsoft.com/en-us/windows/win32/power/pbt-apmpowerstatuschange)
    PowerStatusChange = PBT_APMPOWERSTATUSCHANGE,
    /// Operation is resuming automatically from a low-power state
    /// [See](https://learn.microsoft.com/en-us/windows/win32/power/pbt-apmresumeautomatic)
    ResumeAutomatic = PBT_APMRESUMEAUTOMATIC,
    /// Operation is resuming from a low-power state
    /// [See](https://learn.microsoft.com/en-us/windows/win32/power/pbt-apmresumesuspend)
    ResumeSuspend = PBT_APMRESUMESUSPEND,
    /// System is supspending operation
    /// [See](https://learn.microsoft.com/en-us/windows/win32/power/pbt-apmsuspend)
    Suspend = PBT_APMSUSPEND,
    /// The system power setting has changed
    PowerSettingChange(PowerBroadcastSetting),
}

impl PowerSettingChange {
    /// Safety: Data must be a Option<PowerBroadcastSetting>
    unsafe fn try_parse(event_type: u32, data: *mut c_void) -> Option<Self> {
        match event_type {
            PBT_APMPOWERSTATUSCHANGE => Some(Self::PowerStatusChange),
            PBT_APMRESUMEAUTOMATIC => Some(Self::ResumeAutomatic),
            PBT_APMRESUMESUSPEND => Some(Self::ResumeSuspend),
            PBT_APMSUSPEND => Some(Self::Suspend),
            PBT_POWERSETTINGCHANGE => {
                PowerBroadcastSetting::try_cast(data).map(Self::PowerSettingChange)
            }
            _ => None,
        }
    }
}

impl fmt::Display for PowerSettingChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PowerStatusChange => write!(f, "power status changed"),
            Self::ResumeAutomatic => write!(f, "resume automatic"),
            Self::ResumeSuspend => write!(f, "resume suspend"),
            Self::Suspend => write!(f, "suspend"),
            Self::PowerSettingChange(settings) => write!(f, "power setting change => {settings}"),
        }
    }
}

#[derive(Debug)]
pub enum PowerBroadcastSetting {
    /// See [`PowerCondition`]
    AcDcPowerSource(PowerCondition),
    /// The battery state has changed. The range is between 0-100
    BatteryPercentageRemaining(u32),
    /// The current monitor's DisplayState has changed. See [`DisplayState`]
    ConsoleDisplayState(DisplayState),
    /// The user status associated with any session has changed. See [`UserPresence`]
    GlobalUserPresence(UserPresence),
    /// The system is busy
    IdleBackgroundTask,
    /// The state of the lid has changed (IE: open vs closed). See [`LidswitchState`]
    LidswitchStateChange(LidswitchState),
    /// The primary system monitor has been powered on (true) or off (false). New applications
    /// should use the Display State instead
    MonitorPowerOn(bool),
    /// The battery saver has been turned off (false) or turned on (true)
    PowerSavingStatus(bool),
    //Energy Saver Status See [`EnergySaverStatus`]
    //EnergySaverStatus(EnergySaverStatus),
    /// See [`PowerschemePersonality`]
    PowerschemePersonality(PowerschemePersonality),
    /// The display associated with the applications session has changed state
    /// See [`DisplayState`]
    SessionDisplayStatus(DisplayState),
    /// The user status associated with the applications session has changed. See [`UserPresence`]
    SessionUserPresence(UserPresence),
    /// The system has entered "away mode" (true) or exited "away mode" (false)
    SystemAwayMode(bool),
}

impl fmt::Display for PowerBroadcastSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AcDcPowerSource(condition) => write!(f, "acdc power condition => {condition}"),
            Self::BatteryPercentageRemaining(p) => write!(f, "battery remaining => {p}"),
            Self::ConsoleDisplayState(state) => write!(f, "console display => {state}"),
            Self::GlobalUserPresence(p) => write!(f, "global user precence => {p}"),
            Self::IdleBackgroundTask => write!(f, "idle background task"),
            Self::LidswitchStateChange(lid) => write!(f, "lid switch => {lid}"),
            Self::MonitorPowerOn(status) => write!(f, "monitor power status => {status}"),
            Self::PowerSavingStatus(status) => write!(f, "power savings status => {status}"),
            Self::PowerschemePersonality(p) => write!(f, "power personality => {p}"),
            Self::SessionDisplayStatus(sess) => write!(f, "session display => {sess}"),
            Self::SessionUserPresence(u) => write!(f, "session user presence => {u}"),
            Self::SystemAwayMode(mode) => write!(f, "system away mode {mode}"),
        }
    }
}

impl PowerBroadcastSetting {
    /// Safety: data must be a POWERBROADCAST_SETTING
    unsafe fn try_cast(data: *mut c_void) -> Option<Self> {
        let broadcast = &*(data as *const POWERBROADCAST_SETTING);
        match broadcast.PowerSetting {
            guid if Guid::from(guid) == Guid(GUID_ACDC_POWER_SOURCE) => {
                let condition = PowerCondition::from_u32(*(broadcast.Data.as_ptr() as *const u32));
                condition.map(Self::AcDcPowerSource)
            }
            guid if Guid::from(guid) == Guid(GUID_BATTERY_PERCENTAGE_REMAINING) => {
                let remaining = *(broadcast.Data.as_ptr() as *const u32);
                Some(Self::BatteryPercentageRemaining(remaining))
            }
            guid if Guid::from(guid) == Guid(GUID_CONSOLE_DISPLAY_STATE) => {
                let display = DisplayState::from_u32(*(broadcast.Data.as_ptr() as *const u32));
                display.map(Self::ConsoleDisplayState)
            }
            guid if Guid::from(guid) == Guid(GUID_GLOBAL_USER_PRESENCE) => {
                let presence = UserPresence::from_u32(*(broadcast.Data.as_ptr() as *const u32));
                presence.map(Self::GlobalUserPresence)
            }
            guid if Guid::from(guid) == Guid(GUID_IDLE_BACKGROUND_TASK) => {
                Some(Self::IdleBackgroundTask)
            }
            guid if Guid::from(guid) == Guid(GUID_LIDSWITCH_STATE_CHANGE) => {
                let lid = LidswitchState::from_u32(*(broadcast.Data.as_ptr() as *const u32));
                lid.map(Self::LidswitchStateChange)
            }
            guid if Guid::from(guid) == Guid(GUID_MONITOR_POWER_ON) => {
                let data = *(broadcast.Data.as_ptr() as *const u32);
                let monitor = if data == 0 { false } else { true };
                Some(Self::MonitorPowerOn(monitor))
            }
            guid if Guid::from(guid) == Guid(GUID_POWER_SAVING_STATUS) => {
                let data = *(broadcast.Data.as_ptr() as *const u32);
                let battery_saver = if data == 0 { false } else { true };
                Some(Self::PowerSavingStatus(battery_saver))
            }
            //guid if Guid::from(guid) == Guid(GUID_ENERY_SAVER_STATUS) => unimplemented!(),
            guid if Guid::from(guid) == Guid(GUID_POWERSCHEME_PERSONALITY) => {
                let guid = *(broadcast.Data.as_ptr() as *const windows_sys::core::GUID);
                PowerschemePersonality::try_from_guid(guid).map(Self::PowerschemePersonality)
            }
            guid if Guid::from(guid) == Guid(GUID_SESSION_DISPLAY_STATUS) => {
                let display = DisplayState::from_u32(*(broadcast.Data.as_ptr() as *const u32));
                display.map(Self::SessionDisplayStatus)
            }
            guid if Guid::from(guid) == Guid(GUID_SESSION_USER_PRESENCE) => {
                let presence = UserPresence::from_u32(*(broadcast.Data.as_ptr() as *const u32));
                presence.map(Self::SessionUserPresence)
            }
            guid if Guid::from(guid) == Guid(GUID_SYSTEM_AWAYMODE) => {
                let data = *(broadcast.Data.as_ptr() as *const u32);
                let away = if data == 0 { false } else { true };
                Some(Self::SystemAwayMode(away))
            }
            _ => None,
        }
    }
}

/// [`PowerBroadcastSetting`] AcDc power source has changed
#[derive(FromPrimitive, Debug)]
#[repr(u32)]
pub enum PowerCondition {
    /// The computer is powered by an AC power source
    Ac = 0,
    /// The computer is powered by an onboard battery power source
    Dc = 1,
    /// The computer is powered by a short-term power source device (ie: UPS)
    Hot = 2,
}

impl fmt::Display for PowerCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ac => write!(f, "computer is powred by AC power source"),
            Self::Dc => write!(f, "computer is powred by onboard battery power source"),
            Self::Hot => write!(f, "computer is powred short-term power source"),
        }
    }
}

/// The display has changed state
#[derive(FromPrimitive, Debug)]
#[repr(u32)]
pub enum DisplayState {
    /// The display is off
    Off = 0,
    /// The display is on
    On = 1,
    /// The display is dim
    Dim = 2,
}

impl fmt::Display for DisplayState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => write!(f, "display off"),
            Self::On => write!(f, "display on"),
            Self::Dim => write!(f, "display dim"),
        }
    }
}

/// User activity
#[derive(FromPrimitive, Debug)]
#[repr(u32)]
pub enum UserPresence {
    /// The user is present
    Present = 0,
    /// The user is inactive
    Inactive = 2,
}

impl fmt::Display for UserPresence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Present => write!(f, "user present"),
            Self::Inactive => write!(f, "user inactive"),
        }
    }
}

/// The powerscheme personality has changed
#[derive(Debug)]
pub enum PowerschemePersonality {
    /// High performance mode
    Min,
    /// High power consumption
    Max,
    /// Balanced power consumption
    Typical,
}

impl fmt::Display for PowerschemePersonality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Min => write!(f, "high performance"),
            Self::Max => write!(f, "low performance"),
            Self::Typical => write!(f, "typical performance"),
        }
    }
}

impl PowerschemePersonality {
    fn try_from_guid(guid: windows_sys::core::GUID) -> Option<Self> {
        let guid = Guid::from(guid);
        match guid {
            guid if guid == Guid::from(GUID_MIN_POWER_SAVINGS) => Some(PowerschemePersonality::Min),
            guid if guid == Guid::from(GUID_MAX_POWER_SAVINGS) => Some(PowerschemePersonality::Max),
            guid if guid == Guid::from(GUID_TYPICAL_POWER_SAVINGS) => {
                Some(PowerschemePersonality::Typical)
            }
            _ => None,
        }
    }
}

#[derive(FromPrimitive, Debug)]
#[repr(u32)]
pub enum LidswitchState {
    Closed = 0,
    Open = 1,
}

impl fmt::Display for LidswitchState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "lid opened"),
            Self::Closed => write!(f, "lid closed"),
        }
    }
}

/*
/// The energy saver status has changed
#[derive(FromPrimitive)]
pub enum EnergySaverStatus {
    Off,
    Standard,
    High,
}
*/

/// The status code describing the reason the session state change notification was sent.
/// See: (https://learn.microsoft.com/en-us/windows/win32/termserv/wm-wtssession-change)
#[derive(FromPrimitive, Debug)]
#[repr(u32)]
pub enum SessionChangeType {
    /// Session was connected to the console terminal or RemoteFx session.
    ConsoleConnect = WTS_CONSOLE_CONNECT,
    /// Session was disconnected to the console terminal or RemoteFx session.
    ConsoleDisconnect = WTS_CONSOLE_DISCONNECT,
    /// Session was connected to the remote terminal
    RemoteConnect = WTS_REMOTE_CONNECT,
    /// Session was disconnected to the remote terminal
    RemoteDisconnect = WTS_REMOTE_DISCONNECT,
    /// A user has logged on to the session
    SessionLogon = WTS_SESSION_LOGON,
    /// A user has logged off from the session
    SessionLogoff = WTS_SESSION_LOGOFF,
    /// The session has been locked
    SessionLock = WTS_SESSION_LOCK,
    /// The session has been unlocked
    SessionUnlock = WTS_SESSION_UNLOCK,
    /// The session has changed its remote control status
    SessionRemoteControl = WTS_SESSION_REMOTE_CONTROL,
    /// reserved
    SessionCreate = WTS_SESSION_CREATE,
    /// reserved
    SessionTerminate = WTS_SESSION_TERMINATE,
}

impl fmt::Display for SessionChangeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConsoleConnect => write!(f, "console connected"),
            Self::ConsoleDisconnect => write!(f, "console disconnected"),
            Self::RemoteConnect => write!(f, "remote connected"),
            Self::RemoteDisconnect => write!(f, "remote disconnected"),
            Self::SessionLogon => write!(f, "session logon"),
            Self::SessionLogoff => write!(f, "session logoff"),
            Self::SessionLock => write!(f, "session lock"),
            Self::SessionUnlock => write!(f, "session unlock"),
            Self::SessionRemoteControl => write!(f, "session remote control"),
            Self::SessionCreate => write!(f, "session create"),
            Self::SessionTerminate => write!(f, "session terminate"),
        }
    }
}

/// When processing kernel service messages, a new message code might be added that we are not able
/// to parse
#[derive(Debug)]
pub struct UnsupportedServiceMessage {
    /// The control param that was passed to the HandlerEx routine
    control: u32,
    /// The event_type param that was passed to the HandlerEx routine
    event_type: u32,
}
impl UnsupportedServiceMessage {
    fn new(control: u32, event_type: u32) -> Self {
        Self {
            control,
            event_type,
        }
    }
}
impl fmt::Display for UnsupportedServiceMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let control = self.control;
        let event_type = self.event_type;
        write!(
            f,
            "Unsupported SCM Message (control: {control}, type: {event_type})",
        )
    }
}
impl error::Error for UnsupportedServiceMessage {}

impl ServiceMessageEx /*<D>*/ {
    fn try_parse(
        control: u32,
        event_type: u32,
        event_data: *mut c_void,
    ) -> Result<Self, UnsupportedServiceMessage> {
        match control {
            // Base service messages
            SERVICE_CONTROL_CONTINUE => Ok(Self::Continue),
            SERVICE_CONTROL_INTERROGATE => Ok(Self::Interrogate),
            SERVICE_CONTROL_NETBINDADD => Ok(Self::NetbindAdd),
            SERVICE_CONTROL_NETBINDDISABLE => Ok(Self::NetbindDisable),
            SERVICE_CONTROL_NETBINDENABLE => Ok(Self::NetbindEnable),
            SERVICE_CONTROL_NETBINDREMOVE => Ok(Self::NetbindRemove),
            SERVICE_CONTROL_PARAMCHANGE => Ok(Self::ParamChange),
            SERVICE_CONTROL_PAUSE => Ok(Self::Pause),
            SERVICE_CONTROL_PRESHUTDOWN => Ok(Self::Preshutdown),
            SERVICE_CONTROL_SHUTDOWN => Ok(Self::Shutdown),
            SERVICE_CONTROL_STOP => Ok(Self::Stop),

            // Ex service messages
            SERVICE_CONTROL_DEVICEEVENT => {
                // Safety: the data param is an Option<DEV_BROADCAST_HDR> when the service control
                // message is a DEVICEEVENT
                unsafe { DeviceEvent::try_parse(event_type, event_data) }
                    .ok_or_else(|| UnsupportedServiceMessage {
                        control,
                        event_type,
                    })
                    .map(Self::DeviceEvent)
            }
            SERVICE_CONTROL_HARDWAREPROFILECHANGE => HardwareProfileChange::from_u32(event_type)
                .ok_or_else(|| UnsupportedServiceMessage::new(control, event_type))
                .map(Self::HardwareProfileChange),
            SERVICE_CONTROL_POWEREVENT => {
                // Safety: the data param is an Option<PowerBroadcastSetting> when the service
                // control message is a powerevent
                unsafe { PowerSettingChange::try_parse(event_type, event_data) }
                    .ok_or_else(|| UnsupportedServiceMessage::new(control, event_type))
                    .map(Self::PowerEvent)
            }
            SERVICE_CONTROL_SESSIONCHANGE => SessionChangeType::from_u32(event_type)
                .ok_or_else(|| UnsupportedServiceMessage::new(control, event_type))
                .map(|session_type| {
                    // Safety: This data is from the kernel and docs say it is a
                    // WTS_SESSION_NOTIFICATION
                    let data = unsafe { *(event_data as *const _) };
                    Self::SessionChange(session_type, data)
                }),
            SERVICE_CONTROL_TIMECHANGE => {
                Ok(Self::TimeChange(unsafe { *(event_data as *const _) }))
            }
            SERVICE_CONTROL_TRIGGEREVENT => Ok(Self::TriggerEvent),
            control if control >= 128 && control <= 255 => Ok(Self::UserDefined(
                control as _,
                event_type,
                event_data as usize,
            )),
            _ => Err(UnsupportedServiceMessage::new(control, event_type)),
        }
    }
}

/// A service spawned [`service_macros::start_service_ctrl_dispatcher`] will receive these
/// arguments to the ServiceMain routine
pub type Arguments = Vec<OsString>;

/// Safety: This control handler is called from the context of the Main thread.  The main thread is
/// guarenteed to be alive at least as long as the service routines.
///
/// This is only public because it is used by the [`service_macros::start_service_ctrl_dispatcher`]
#[doc(hidden)]
pub unsafe extern "system" fn service_control_message_handler(
    control: u32,
    event_type: u32,
    event_data: *mut c_void,
    context: *mut c_void,
) -> u32 {
    // NOTE that we must only construct a "borrowed" version of the message. While parsing we leak
    // the box. When the consumer end of the event queue consumes the message, the api enforces
    // that they will consume an "Owned" version of the message, for which the message will be
    // dropped
    let m = ServiceMessageEx::try_parse(control, event_type, event_data);
    match m {
        Ok(m) => {
            let context = &mut *(context as *mut ServiceMessageState);
            context.messages.push(m);
            if let Some(waker) = context.waker.lock().as_ref() {
                waker.wake_by_ref();
                NO_ERROR
            } else {
                warn!("no waker available yet");
                NO_ERROR
            }
        }
        Err(error) => {
            error!(?error, "failed to parse service message");
            NO_ERROR
        }
    }
}

#[derive(Default)]
pub struct ServiceMessageState {
    /// A queue of messages waiting to be received from the service stream. NOTE should we use an
    /// array queue instead and drop old messages IE: back pressure? (probably)
    messages: SegQueue<ServiceMessageEx>,
    /// The "Waker" for when we have a new message ready
    waker: Mutex<Option<Waker>>,
}

/// A stream of service messages. The message emit from the applications "Main" thread, which is
/// distinguished from the "ServiceMain" thread.  The kernel guarentees tht the "Main" thread will
/// live at least as long as all "ServiceMain" threads. Therefore, we treat these threads as a
/// "Scoped" thread and we allow data on the Main stack to be referenced by the "ServiceMain"
/// thread.
#[repr(C)]
#[derive(Default)]
pub struct ServiceMessageStream {
    state: Arc<ServiceMessageState>,
}

impl ServiceMessageStream {
    pub fn state(&self) -> *const ServiceMessageState {
        Arc::as_ptr(&self.state)
    }
}

impl Stream for ServiceMessageStream {
    type Item = ServiceMessageEx;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut waker = self.state.waker.lock();

        // Diagnostic stuff
        let pending = self.state.messages.len();
        debug!(pending, "pending SCM messages");

        // Maybe the caller a message
        match self.state.messages.pop() {
            Some(ServiceMessageEx::Stop)
            | Some(ServiceMessageEx::Preshutdown)
            | Some(ServiceMessageEx::Shutdown) => Poll::Ready(None),
            Some(message) => Poll::Ready(Some(message)),
            None => {
                // Some waker accounting
                let new_waker = cx.waker();
                *waker = match waker.take() {
                    Some(old_waker) => match old_waker.will_wake(new_waker) {
                        true => Some(old_waker),
                        false => Some(new_waker.clone()),
                    },
                    None => Some(new_waker.clone()),
                };
                Poll::Pending
            }
        }
    }
}
