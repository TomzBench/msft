//! The StatusHandle used to communicate with windows SCM

use crate::message::{service_control_message_handler, ServiceMessageStream};
use bitflags::bitflags;
use std::io;
use std::os::windows::prelude::{AsRawHandle, RawHandle};
use tracing::error;
use windows_sys::Win32::System::{Services::*, SystemServices::*};

bitflags! {
    /// The type of service. Must set when calling SetServiceStatus.
    ///
    /// [See also](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_status)
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ServiceType: u32 {
        /// This service is a file system driver
        const FileSystemDriver = SERVICE_FILE_SYSTEM_DRIVER;
        /// This service is a device driver
        const KernelDriver = SERVICE_KERNEL_DRIVER;
        /// This service runs in its own proces
        const Win32OwnProcess = SERVICE_WIN32_OWN_PROCESS;
        /// This service shares a process with other services
        const Win32ShareProcess = SERVICE_WIN32_SHARE_PROCESS;
        /// This service runs in its own process under the logged-on user account
        const UserOwnProcess = SERVICE_USER_OWN_PROCESS;
        /// This service shares a process with one or more other services that run under the logged-on
        /// user account
        const UserShareProcess = SERVICE_USER_SHARE_PROCESS;
        /// [See also](https://learn.microsoft.com/en-us/windows/win32/services/interactive-services)
        const InteractiveProcess = SERVICE_INTERACTIVE_PROCESS;
    }
}

bitflags! {
    /// The Current state of the service.
    ///
    /// [See also](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_status)
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CurrentState: u32 {
        /// The service continue is pending
        const ContinuePending = SERVICE_CONTINUE_PENDING;
        /// The service pause is pending
        const ServicePausePending = SERVICE_PAUSE_PENDING;
        /// The service is paused
        const ServicePaused = SERVICE_PAUSED;
        /// The service is running
        const ServiceRunning = SERVICE_RUNNING;
        /// The service is starting
        const ServiceStartPending = SERVICE_START_PENDING;
        /// The service is stopping
        const ServiceStopPending = SERVICE_STOP_PENDING;
        /// The service is not running
        const ServiceStopped = SERVICE_STOPPED;
    }
}

bitflags! {
    /// The control codes the service accepts and process in its handler function.  A user
    /// interface process can control a service by specifying a control command in the
    /// ControlService or ControlServiceEx function. By default, all services accept the
    /// SERVICE_CONTROL_INTERROGATE value.
    ///
    /// To accept the SERVICE_CONTROL_DEVICEEVENT value, the service must register to receive a
    /// device event by useing the RegisterDeviceNotification function.
    pub struct ServiceControlAccept: u32 {
        /// This control code allows the service to receive all the NETBIND service messages
        const NETBINDCHANGE = SERVICE_ACCEPT_NETBINDCHANGE;
        /// This control code allows the service to receive PARAMCHANGE notifcations
        const PARAMCHANGE = SERVICE_ACCEPT_PARAMCHANGE;
        /// This control code allows the service to receive PAUSE and CONTINUE notifications
        const PAUSE_CONTINUE = SERVICE_ACCEPT_PAUSE_CONTINUE;
        /// This control code enables the service to receive PRESHUTDOWN notifications
        const PRESHUTDOWN = SERVICE_ACCEPT_PRESHUTDOWN;
        /// This control code allows the service to receive SHUTDOWN notifications
        const SHUTDOWN = SERVICE_ACCEPT_SHUTDOWN;
        /// This control code allows the service to receive STOP notifications
        const STOP = SERVICE_ACCEPT_STOP;

        /// This control code allows the service to receive HARDWAREPROFILECHANGE notification
        const HARDWAREPROFILECHANGE = SERVICE_ACCEPT_HARDWAREPROFILECHANGE;
        /// This control code allows the service to receive POWEREVENT notifications
        const POWEREVENT = SERVICE_ACCEPT_POWEREVENT;
        /// This control code allows the service to receive SESSION change notifications
        const SESSIONCHANGE = SERVICE_ACCEPT_SESSIONCHANGE;
        /// This control code allows the service to receive TIMECHANGE notifications
        const TIMECHANGE = SERVICE_ACCEPT_TIMECHANGE;
        /// This control code allows the service to receive TRIGGEREVENT notifications
        const TRIGGEREVENT = SERVICE_ACCEPT_TRIGGEREVENT;
        // /// This control code allows the service to receive USERMODEREBOOT notifications
        // const USERMODEREBOOT = SERVICE_ACCEPT_USERMODEREBOOT;
    }
}

/// TODO
pub struct StatusHandle {
    handle: isize,
    status: SERVICE_STATUS,
}
impl AsRawHandle for StatusHandle {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle as _
    }
}

impl StatusHandle {
    /// Call RegisterServiceCtrlHandlerExW. This method expects caller to initialize a stream. The
    /// stream is passed to the registration as context data which internally will drive the stream
    /// of SCM messages.
    ///
    /// [See](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-registerservicectrlhandlerexw)
    pub fn new(name: *const u16, stream: &ServiceMessageStream) -> io::Result<Self> {
        let result = unsafe {
            RegisterServiceCtrlHandlerExW(
                name,
                Some(service_control_message_handler),
                stream.state() as _,
            )
        };
        match result {
            0 => Err(io::Error::last_os_error()),
            handle => Ok(StatusHandle {
                handle,
                status: unsafe { std::mem::zeroed() },
            }),
        }
    }

    pub fn set_service_type(&mut self, ty: ServiceType) -> &mut Self {
        self.status.dwServiceType = ty.bits();
        self
    }

    pub fn set_current_state(&mut self, state: CurrentState) -> &mut Self {
        self.status.dwCurrentState = state.bits();
        self
    }

    pub fn set_control_accept(&mut self, accept: ServiceControlAccept) -> &mut Self {
        self.status.dwControlsAccepted = accept.bits();
        self
    }

    pub fn set_wait_hint(&mut self, hints: u32) -> &mut Self {
        self.status.dwWaitHint = hints;
        self
    }

    pub fn set_check_point(&mut self, check_point: u32) -> &mut Self {
        self.status.dwCheckPoint = check_point;
        self
    }

    pub fn set_exit_code(&mut self, exit_code: u32) -> &mut Self {
        self.status.dwWin32ExitCode = exit_code;
        self
    }

    pub fn set_service_exit_code(&mut self, exit_code: u32) -> &mut Self {
        self.status.dwServiceSpecificExitCode = exit_code;
        self
    }

    /// Set the status structure containing ServiceType, ServiceState, ControlsAccepted, 2 exit
    /// codes, a "progress bar" type and a "wait hint" for timeout accounting
    ///
    /// [See
    /// also:](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-setservicestatus)
    pub fn set_status(&self) -> io::Result<()> {
        match unsafe { SetServiceStatus(self.handle as _, &self.status as *const _) } {
            0 => {
                let error = io::Error::last_os_error();
                error!(?error, "Failed to set service status");
                Err(error)
            }
            _ => Ok(()),
        }
    }
}
