/// Generates a typed async RPC interface between a network task and its handle.
///
/// `saved_rpc!` defines:
/// - A **trait** (e.g. `SavedNetworkApi`) of async methods implemented by the network.
/// - An **enum** (e.g. `SavedNetworkRpc`) used as messages over an `mpsc` channel.
/// - **Handle methods** that send those RPCs and await replies.
/// - A **dispatcher** on the network to call the real trait methods.
///
/// ### Example
/// ```ignore
/// saved_rpc! {
///     trait SavedNetworkApi for SavedNetwork {
///         async fn dial(addr: Multiaddr) -> SavedResult<()>;
///     }
///     handle = SavedHandle,
///     cmd_enum = SavedNetworkRpc,
///     result = SavedResult<()>,
///     error  = crate::error::SavedError
/// }
/// ```
///
/// This creates a fully wired request/response layer over `tokio::mpsc` + `oneshot`,
/// so `SavedHandle::dial()` calls `SavedNetwork::dial()` asynchronously and type-safely.

#[macro_export]
macro_rules! saved_rpc {
    (
        // Trait that defines the async API surface you want to expose
        trait $trait_name:ident for $net_ty:ty {
            $(
                async fn $fn_name:ident ( $( $arg:ident : $arg_ty:ty ),* $(,)? ) -> $ret:ty ;
            )*
        }

        // Types/targets to generate for
        handle = $handle_ty:ty,
        cmd_enum = $cmd_enum:ident,

        // Concrete result and error types
        result = $result_ty:ty,
        error  = $error_path:path
    ) => {
        // --- 0) Define the trait ---
        pub trait $trait_name {
            $(
                async fn $fn_name(&mut self, $( $arg: $arg_ty ),* ) -> $ret;
            )*
        }

        #[allow(non_camel_case_types)]
        pub enum $cmd_enum {
            $(
                $fn_name {
                    $( $arg: $arg_ty, )*
                    rsp: tokio::sync::oneshot::Sender<$ret>,
                },
            )*
        }

        impl $handle_ty {
            $(
                pub async fn $fn_name(&self, $( $arg: $arg_ty ),* ) -> $ret {
                    use tokio::sync::oneshot;

                    let (tx, rx) = oneshot::channel();
                    self.cmd_tx
                        .send($cmd_enum::$fn_name { $( $arg, )* rsp: tx })
                        .await?;

                    rx.await.map_err(|_| {
                        use $error_path as __SavedError;
                        let e: __SavedError = __SavedError::OneshotCanceled { rpc: stringify!($fn_name) };
                        e
                    })?
                }
            )*
        }

        impl $net_ty {
            pub(crate) async fn dispatch_rpc(
                &mut self,
                cmd: $cmd_enum
            ) -> $result_ty {
                match cmd {
                    $(
                        $cmd_enum::$fn_name { $( $arg, )* rsp } => {
                            let res = <$net_ty as $trait_name>::$fn_name(self, $( $arg ),*).await;
                            let _ = rsp.send(res);
                        }
                    )*
                }
                Ok(())
            }
        }
    }
}
