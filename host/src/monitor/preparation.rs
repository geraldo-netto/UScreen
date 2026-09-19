//! Prepare one route with its slot's credential, independently of other devices.
use super::*;

pub(super) struct Preparation {
    pub serial: String,
    pub ports: (u16, u16),
    pub token: Result<Option<String>>,
    pub token_dir: Option<PathBuf>,
    pub instance: u32,
    pub adb: String,
    pub auto_launch: bool,
    pub ticket: Option<launch_policy::Ticket>,
    pub retry: RelaunchBackoff,
}
impl Preparation {
    pub async fn run(mut self) -> Mutation {
        let token = self.token.and_then(|token| {
            credentials::publish(self.token_dir.as_deref(), self.instance, token.as_deref())?;
            Ok(token)
        });
        let ready = match token {
            Ok(token) => {
                let request = TabletConnection {
                    serial: &self.serial,
                    video_port: self.ports.0,
                    input_port: self.ports.1,
                    auto_launch: false,
                    token: token.as_deref(),
                    adb: &self.adb,
                };
                let ready = prepare_route(&request, &mut self.retry).await;
                if ready && self.auto_launch && self.ticket.is_some_and(|ticket| ticket.take()) {
                    launch_app_using(&self.serial, token.as_deref(), &self.adb).await;
                }
                ready
            }
            Err(error) => {
                warn!("Attachment credential preparation failed: {error}");
                self.retry.allow(Instant::now());
                false
            }
        };
        Mutation::Prepared {
            ready,
            retry: self.retry,
        }
    }
}

async fn prepare_route(request: &TabletConnection<'_>, retry: &mut RelaunchBackoff) -> bool {
    if !request.prepare(retry, Instant::now()).await {
        return false;
    }
    if is_fake_serial(request.serial) {
        return true;
    }
    let delivered = redeliver_token_using(request.serial, request.token, request.adb).await;
    if request.token.is_some() && !delivered {
        retry.allow(Instant::now());
        return false;
    }
    true
}
