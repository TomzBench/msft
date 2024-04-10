//! service.rs

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote, ToTokens};
use syn::{
    parse::Parser, punctuated::Punctuated, Attribute, Error, Expr, ExprLit, ExprPath, ExprTuple,
    FnArg, ItemFn, Lit, LitInt, LitStr, MetaNameValue, Pat, PatType, Path, Result, Token, Type,
    TypePath,
};

/// For collecting the service arguments
struct Service {
    name: LitStr,
    service: Path,
    attrs: Vec<Attribute>,
}

#[derive(Default)]
struct Meta {
    name: Option<LitStr>,
    worker_threads: Option<LitInt>,
    mt: bool,
}

/// A general message displayed at the callsite when the user supplied invalid tuple
fn err_fold(prop: &str) -> Error {
    Error::new(
        Span::call_site(),
        format!("Expected a tuple with service name and service method. Missing {prop}"),
    )
}

fn err_missing_arg<T: ToTokens>(arg: &'static str, toks: T) -> Error {
    Error::new_spanned(
        toks,
        format!("Service function must accept {arg} parameter"),
    )
}

/// Collect the service arguments into a Vec<Service>
fn fold(mut vec: Vec<Service>, expr: ExprTuple) -> Result<Vec<Service>> {
    // Consume the tuples
    let mut iter = expr.elems.into_iter();
    // The first element in the tuple should be the service name as a LitStr
    let name = match iter.next() {
        Some(Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        })) => Ok(s),
        _ => Err(err_fold("name")),
    }?;
    // The second element in the tuple should be the service routine as a Path
    let service = match iter.next() {
        Some(Expr::Path(ExprPath { path, .. })) => Ok(path),
        _ => Err(err_fold("service")),
    }?;
    vec.push(Service {
        name,
        service,
        attrs: expr.attrs,
    });
    Ok(vec)
}

fn match_name(mut meta: Meta, expr: Expr) -> Meta {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => {
            meta.name = Some(s);
            meta
        }
        _ => meta,
    }
}

fn match_mt(mut meta: Meta, expr: Expr) -> Meta {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Bool(b), ..
        }) => {
            meta.mt = b.value;
            meta
        }
        _ => meta,
    }
}

fn match_worker_threads(mut meta: Meta, expr: Expr) -> Meta {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) => {
            meta.worker_threads = Some(i);
            meta
        }
        _ => meta,
    }
}

fn fold_meta(meta: Meta, expr: MetaNameValue) -> Meta {
    match expr.path.get_ident() {
        Some(ident) if ident == "name" => match_name(meta, expr.value),
        Some(ident) if ident == "mt" => match_mt(meta, expr.value),
        Some(ident) if ident == "worker_threads" => match_worker_threads(meta, expr.value),
        _ => meta,
    }
}

fn make_find_function_argument(name: &'static str) -> impl Fn(&FnArg) -> Option<(&Pat, &Path)> {
    move |arg| -> Option<(&Pat, &Path)> {
        match arg {
            FnArg::Typed(PatType { pat, ty, .. }) => match ty.as_ref() {
                Type::Path(TypePath { path, .. }) => {
                    let ident = path.segments.last().map(|seg| &seg.ident).cloned()?;
                    if ident == name {
                        Some((&pat, &path))
                    } else {
                        None
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }
}
fn find_arg<'a>(name: &'static str, func: &'a ItemFn) -> Result<(&'a Pat, &'a Path)> {
    func.sig
        .inputs
        .iter()
        .find_map(make_find_function_argument(name))
        .ok_or_else(|| err_missing_arg(name, func.clone()))
}

pub fn expand_start_service_ctrl_dispatcher(toks: TokenStream) -> Result<TokenStream2> {
    let parsed = Parser::parse(Punctuated::<ExprTuple, Token![,]>::parse_terminated, toks)?;
    let nservices = parsed.iter().len();

    // Parse the tuple for populating service array
    let folded = parsed
        .into_iter()
        .try_fold(Vec::with_capacity(nservices), fold)?;

    // Generate *const u16 namse for service array
    let names = folded.iter().map(|service| {
        let name = &service.name;
        let arg = format_ident!("SERVICE_{}", name.value().to_uppercase().replace(" ", "_"));
        let attrs = service.attrs.iter();
        quote! {
            #(#attrs)*
            const #arg: *const u16 = windows_sys::w!(#name);
        }
    });

    // Generate SERVICE_TABLE_ENTRYW array items
    let table_items = folded.iter().map(|service| {
        let name = &service.name;
        let arg = format_ident!("SERVICE_{}", name.value().to_uppercase().replace(" ", "_"));
        let func = &service.service;
        quote! {
            windows_sys::Win32::System::Services::SERVICE_TABLE_ENTRYW {
                lpServiceName: #arg as _,
                lpServiceProc: Some(#func)
            }
        }
    });

    // Create the SERVICE_TABLE_ENTRYW array + a null terminator entry
    let table = quote! {
        let table = [
            #(#table_items),*,
            windows_sys::Win32::System::Services::SERVICE_TABLE_ENTRYW {
                lpServiceName: std::ptr::null_mut(),
                lpServiceProc: None,
            }
        ];
    };

    // Run the service
    let run_service = quote! {
        let result = unsafe { windows_sys::Win32::System::Services::StartServiceCtrlDispatcherW(&table as *const _) };
        if 0 == result {
            let err = std::io::Error::last_os_error();
            tracing::error!("Failed to start service table {:?}", err);
        }
    };

    // TODO generate service routines
    Ok(quote! {
        #(#names)*
        #table
        #run_service
    })
}

pub fn expand_service(attrs: TokenStream, toks: TokenStream) -> Result<TokenStream2> {
    // Parse service options
    let Meta {
        name,
        mt,
        worker_threads,
    } = Parser::parse(
        Punctuated::<MetaNameValue, Token![,]>::parse_terminated,
        attrs,
    )?
    .into_iter()
    .fold(Meta::default(), fold_meta);

    // Parse the original function
    let orig = syn::parse::<ItemFn>(toks)?;
    // NOTE we're not sure if our service Arguments are in scope or not, so we reuse the callers
    // Arguments as defined in their function. This provents a compiler warning because we rewrite
    // away their arguments and reuse it in the body. However, if we were to use the fully
    // qualified path, then they would get a compiler warning saying the argument is not used,
    // because we re-wrote the argument away (and moved it into the body).
    let (stream_pat, stream_path) = find_arg("ServiceMessageStream", &orig)?;
    let (status_handle_pat, status_handle_path) = find_arg("StatusHandle", &orig)?;

    // We construct the service handle, Vec<OsString>, and a stream of SCM messages. Note that the
    // names __dwnumserviceargs and __lpserviceargvectors must match the final construction of the
    // fn arguments
    let init_os_service_args = find_arg("Arguments", &orig).map(|(pat, path)| {
        quote! {
            let #pat: #path = (0..__dwnumserviceargs).map(|i| {
                let p: *mut *mut u16 = __lpserviceargvectors.offset(i as isize);
                msft_service::util::wchar::from_wide(*p)
            }).collect();
        }
    })?;

    // Create a stream which will be registered with the status handle
    let init_stream = quote! {
        let #stream_pat: #stream_path = Default::default();
    };

    // Create a status handle and register the stream.
    // return an Err(type)
    let init_handle = quote! {
        let #status_handle_pat = match #status_handle_path::new(
            SERVICE_NAME,
            &#stream_pat) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!("Failed to register status handle {:?}", error);
                panic!("Failed to register status handle {:?}", error);
            }
        };
    };

    // Initialize a string for registering the ServiceStatusHandle
    let init_service_name = quote! {
        const SERVICE_NAME: *const u16 = windows_sys::w!(#name);
    };

    let rt = if mt {
        // TODO get the number of threads via runtime... ie windows_sys::Info....
        let nworkers = worker_threads
            .map(|int| int.into_token_stream())
            .unwrap_or_else(|| {
                quote! {{
                    use windows_sys::Win32::System::SystemInformation::{SYSTEM_INFO, GetSystemInfo};
                    let mut info = std::mem::zeroed::<SYSTEM_INFO>();
                    unsafe { GetSystemInfo(&mut info as _) };
                    info.dwNumberOfProcessors << 1
                }}
            });
        quote! {{
            let nworkers: u32 = #nworkers as _;
            match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(nworkers as _)
                .build() {
                    Ok(rt) => rt,
                    Err(e) => {
                        tracing::error!("Failed to build tokio runtime {:?}", e);
                        panic!("Failed to build tokio runtime {:?}", e);
                    }
                }
        }}
    } else {
        quote! {
            match tokio::runtime::Builder::new_current_thread().build() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("Failed to build tokio runtime {:?}", e);
                    panic!("Failed to build tokio runtime {:?}", e);
                }
            }
        }
    };

    // Get parts of the original function (visibility, name, block statements). For to reconstruct
    // a new function
    let vis = &orig.vis;
    let fn_name_orig = &orig.sig.ident;
    let stmts = &orig.block.stmts;

    // If we are async we consruct a tokio runtime and run the users statements in an async block.
    // If we are not async then we simply render the statements. The caller is expected to setup
    // the runtime to setup the SCM message handling because the SCM messages are a stream!
    if orig.sig.asyncness.is_some() {
        Ok(quote! {
            #vis unsafe extern "system" fn #fn_name_orig (
                __dwnumserviceargs: u32,
                __lpserviceargvectors: *mut *mut u16) {
                #init_service_name
                #init_os_service_args
                #init_stream
                #init_handle
                let runtime = #rt.block_on(async move {
                    #(#stmts)*
                });
            }
        })
    } else {
        Ok(quote! {
            #vis unsafe extern "system" fn #fn_name_orig (
                __dwnumserviceargs: u32,
                __lpserviceargvectors: *mut *mut u16) {
                #init_service_name
                #init_os_service_args
                #init_stream
                #init_handle
                #(#stmts)*
            }
        })
    }
}
