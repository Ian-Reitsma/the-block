#![deny(warnings)]

use crate::rpc::{RpcClient, WalletQosError, WalletQosEvent};
use crate::tx::{generate_keypair, sign_tx, FeeLane, RawTxPayload};
use anyhow::{anyhow, Context, Result};
use clap::Subcommand;
use crypto::session::SessionKey;
use hex;
use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::HashMap;
#[cfg(feature = "quantum")]
use std::fs::File;
use std::io::{self, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FEE_FLOOR_CACHE_TTL: Duration = Duration::from_secs(10);

static FEE_FLOOR_CACHE: Lazy<Mutex<HashMap<String, CacheEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

struct CacheEntry {
    floor: u64,
    fetched_at: Instant,
}

#[derive(Subcommand)]
pub enum WalletCmd {
    /// Generate Ed25519 and Dilithium keys in parallel and export keystore
    Gen {
        #[arg(long, default_value = "keystore.json")]
        out: String,
    },
    /// Show available wallet commands
    Help,
    /// List balances for all known tokens
    Balances,
    /// Send tokens to an address with optional ephemeral source
    Send {
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u64,
        #[arg(long, default_value_t = 0)]
        fee: u64,
        #[arg(long, default_value_t = 0)]
        nonce: u64,
        #[arg(long, default_value_t = 100)]
        pct_ct: u8,
        #[arg(long)]
        memo: Option<String>,
        #[arg(long, default_value = "consumer")]
        lane: String,
        #[arg(long, default_value = "http://127.0.0.1:26658")]
        rpc: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        ephemeral: bool,
        #[arg(long)]
        auto_bump: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
        #[arg(long = "lang")]
        lang: Option<String>,
    },
    /// Generate a session key with specified TTL in seconds
    Session {
        #[arg(long, default_value_t = 3600)]
        ttl: u64,
    },
    /// Broadcast a meta-transaction signed by a session key
    MetaSend {
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u64,
        #[arg(long)]
        session_sk: String,
    },
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildTxStatus {
    Ready,
    NeedsConfirmation,
    Cancelled,
}

#[derive(Debug, Serialize)]
pub struct BuildTxReport {
    pub status: BuildTxStatus,
    pub user_fee: u64,
    pub effective_fee: u64,
    pub fee_floor: u64,
    pub lane: String,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<RawTxPayload>,
    pub auto_bumped: bool,
    pub forced: bool,
}

enum WalletEventKind {
    Warning,
    Override,
}

impl WalletEventKind {
    fn as_str(&self) -> &'static str {
        match self {
            WalletEventKind::Warning => "warning",
            WalletEventKind::Override => "override",
        }
    }
}

pub fn handle(cmd: WalletCmd) {
    match cmd {
        WalletCmd::Gen { out } => {
            #[cfg(feature = "quantum")]
            {
                use std::thread;
                use wallet::pq::generate as pq_generate;
                use wallet::Wallet;
                let ed_handle = thread::spawn(|| Wallet::generate());
                let pq_handle = thread::spawn(|| pq_generate());
                let ed = ed_handle.join().expect("ed25519");
                let (pq_pk, pq_sk) = pq_handle.join().expect("dilithium");
                let mut f = File::create(&out).expect("write");
                let json = serde_json::json!({
                    "ed25519_pub": hex::encode(ed.public_key().to_bytes()),
                    "dilithium_pub": hex::encode(pq_pk.as_bytes()),
                    "dilithium_sk": hex::encode(pq_sk.as_bytes()),
                });
                f.write_all(json.to_string().as_bytes()).expect("write");
                println!("exported keystore to {}", out);
            }
            #[cfg(not(feature = "quantum"))]
            {
                println!(
                    "quantum feature not enabled; cannot export keystore to {}",
                    out
                );
            }
        }
        WalletCmd::Help => {
            println!(
                "wallet commands:
  gen --out <FILE>    Generate key material
  help                Show this message"
            );
        }
        WalletCmd::Balances => {
            println!(
                "token balances:
  CT: 0
  IT: 0"
            );
        }
        WalletCmd::Send {
            to,
            amount,
            fee,
            nonce,
            pct_ct,
            memo,
            lane,
            rpc,
            from,
            ephemeral,
            auto_bump,
            force,
            json,
            lang,
        } => {
            if auto_bump && force {
                eprintln!("--auto-bump and --force cannot be combined");
                return;
            }
            let language = Language::detect(&lang);
            let localizer = Localizer::new(language);
            let memo_bytes = memo.unwrap_or_default().into_bytes();
            let lane = match parse_lane(&lane) {
                Ok(lane) => lane,
                Err(err) => {
                    if json {
                        let payload = serde_json::json!({
                            "status": "error",
                            "message": err.to_string(),
                        });
                        println!("{}", payload);
                    } else {
                        eprintln!("{}", err);
                    }
                    return;
                }
            };
            let mut from_addr = from.unwrap_or_else(|| "wallet".to_string());
            let mut ephemeral_notice = None;
            if ephemeral {
                let (_, pk_bytes) = generate_keypair();
                from_addr = hex::encode(&pk_bytes);
                if !json {
                    ephemeral_notice = Some(localizer.ephemeral_notice(&from_addr, amount, &to));
                }
            }
            let client = RpcClient::from_env();
            match build_tx(
                &client,
                &rpc,
                lane,
                &from_addr,
                &to,
                amount,
                fee,
                pct_ct,
                nonce,
                &memo_bytes,
                auto_bump,
                force,
                json,
                &localizer,
            ) {
                Ok(report) => {
                    if json {
                        match serde_json::to_string_pretty(&report) {
                            Ok(text) => println!("{}", text),
                            Err(err) => eprintln!("failed to encode json: {err}"),
                        }
                        return;
                    }
                    for warning in &report.warnings {
                        println!("{}", warning);
                    }
                    if let Some(message) = ephemeral_notice.take() {
                        println!("{}", message);
                    }
                    match report.status {
                        BuildTxStatus::Ready => {
                            if let Some(payload) = report.payload {
                                println!(
                                    "{}",
                                    localizer.success_message(
                                        &from_addr,
                                        &to,
                                        amount,
                                        report.effective_fee,
                                        report.fee_floor,
                                        lane,
                                        report.auto_bumped,
                                        report.forced,
                                    )
                                );
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&payload)
                                        .unwrap_or_else(|_| "{}".into())
                                );
                            }
                        }
                        BuildTxStatus::NeedsConfirmation => {
                            println!("{}", localizer.needs_confirmation());
                        }
                        BuildTxStatus::Cancelled => {
                            println!("{}", localizer.cancelled());
                        }
                    }
                }
                Err(err) => {
                    if json {
                        let payload = serde_json::json!({
                            "status": "error",
                            "message": err.to_string(),
                        });
                        println!("{}", payload);
                    } else {
                        eprintln!("wallet send failed: {err}");
                    }
                }
            }
        }
        WalletCmd::Session { ttl } => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let sk = SessionKey::generate(now + ttl);
            println!(
                "session key issued pk={} expires_at={}",
                hex::encode(&sk.public_key),
                sk.expires_at
            );
            println!("secret={}", hex::encode(sk.secret.to_bytes()));
        }
        WalletCmd::MetaSend {
            to,
            amount,
            session_sk,
        } => {
            let sk_bytes = hex::decode(session_sk).expect("hex");
            let payload = RawTxPayload {
                from_: "meta".into(),
                to,
                amount_consumer: amount,
                amount_industrial: 0,
                fee: 0,
                pct_ct: 100,
                nonce: 0,
                memo: Vec::new(),
            };
            if let Some(tx) = sign_tx(&sk_bytes, &payload) {
                println!(
                    "meta-tx signed {}",
                    hex::encode(bincode::serialize(&tx).unwrap())
                );
            } else {
                println!("invalid session key");
            }
        }
    }
}

enum PromptDecision {
    Auto,
    Force,
    Cancel,
}

pub fn build_tx_default_locale(
    client: &RpcClient,
    rpc: &str,
    lane: FeeLane,
    from: &str,
    to: &str,
    amount: u64,
    fee: u64,
    pct_ct: u8,
    nonce: u64,
    memo: &[u8],
    auto_bump: bool,
    force: bool,
    json: bool,
) -> Result<BuildTxReport> {
    let localizer = Localizer::new(Language::En);
    build_tx(
        client, rpc, lane, from, to, amount, fee, pct_ct, nonce, memo, auto_bump, force, json,
        &localizer,
    )
}

pub fn build_tx(
    client: &RpcClient,
    rpc: &str,
    lane: FeeLane,
    from: &str,
    to: &str,
    amount: u64,
    fee: u64,
    pct_ct: u8,
    nonce: u64,
    memo: &[u8],
    auto_bump: bool,
    force: bool,
    json: bool,
    localizer: &Localizer,
) -> Result<BuildTxReport> {
    let floor = cached_fee_floor(client, rpc, lane)
        .with_context(|| format!("fetching mempool stats from {}", rpc))?;
    let mut warnings = Vec::new();
    let lane_label = lane.as_str().to_string();
    let mut status = BuildTxStatus::Ready;
    let mut effective_fee = fee;
    let mut auto_bumped_flag = false;
    let mut forced_flag = false;

    if fee < floor {
        warnings.push(localizer.warning_message(lane, fee, floor));
        if force {
            forced_flag = true;
            if !json {
                println!("{}", localizer.force_confirmation(fee, floor));
            }
            record_wallet_event(
                client,
                rpc,
                WalletEventKind::Override,
                lane,
                fee,
                floor,
                json,
            );
        } else if auto_bump {
            effective_fee = floor;
            auto_bumped_flag = true;
            if !json {
                println!("{}", localizer.auto_bump_confirmation(floor));
            }
            record_wallet_event(
                client,
                rpc,
                WalletEventKind::Warning,
                lane,
                effective_fee,
                floor,
                json,
            );
        } else if json {
            status = BuildTxStatus::NeedsConfirmation;
            record_wallet_event(
                client,
                rpc,
                WalletEventKind::Warning,
                lane,
                fee,
                floor,
                json,
            );
            return Ok(BuildTxReport {
                status,
                user_fee: fee,
                effective_fee: fee,
                fee_floor: floor,
                lane: lane_label,
                warnings,
                payload: None,
                auto_bumped: false,
                forced: false,
            });
        } else {
            println!("{}", warnings.last().unwrap());
            let decision = prompt_user(localizer)?;
            match decision {
                PromptDecision::Auto => {
                    effective_fee = floor;
                    auto_bumped_flag = true;
                    println!("{}", localizer.auto_bump_confirmation(floor));
                    record_wallet_event(
                        client,
                        rpc,
                        WalletEventKind::Warning,
                        lane,
                        effective_fee,
                        floor,
                        json,
                    );
                }
                PromptDecision::Force => {
                    forced_flag = true;
                    println!("{}", localizer.force_confirmation(fee, floor));
                    record_wallet_event(
                        client,
                        rpc,
                        WalletEventKind::Override,
                        lane,
                        fee,
                        floor,
                        json,
                    );
                }
                PromptDecision::Cancel => {
                    record_wallet_event(
                        client,
                        rpc,
                        WalletEventKind::Warning,
                        lane,
                        fee,
                        floor,
                        json,
                    );
                    return Ok(BuildTxReport {
                        status: BuildTxStatus::Cancelled,
                        user_fee: fee,
                        effective_fee: fee,
                        fee_floor: floor,
                        lane: lane_label,
                        warnings,
                        payload: None,
                        auto_bumped: false,
                        forced: false,
                    });
                }
            }
        }
    }

    let pct_ct = pct_ct.min(100);
    let (amount_consumer, amount_industrial) = match lane {
        FeeLane::Consumer => (amount, 0),
        FeeLane::Industrial => (0, amount),
    };
    let payload = RawTxPayload {
        from_: from.to_string(),
        to: to.to_string(),
        amount_consumer,
        amount_industrial,
        fee: effective_fee,
        pct_ct,
        nonce,
        memo: memo.to_vec(),
    };

    Ok(BuildTxReport {
        status,
        user_fee: fee,
        effective_fee,
        fee_floor: floor,
        lane: lane_label,
        warnings,
        payload: Some(payload),
        auto_bumped: auto_bumped_flag,
        forced: forced_flag,
    })
}

fn cached_fee_floor(client: &RpcClient, rpc: &str, lane: FeeLane) -> Result<u64> {
    let key = format!("{}::{}", rpc, lane.as_str());
    if let Some(floor) = {
        let cache = FEE_FLOOR_CACHE.lock().unwrap();
        cache.get(&key).and_then(|entry| {
            if entry.fetched_at.elapsed() < FEE_FLOOR_CACHE_TTL {
                Some(entry.floor)
            } else {
                None
            }
        })
    } {
        return Ok(floor);
    }
    let stats = client
        .mempool_stats(rpc, lane)
        .with_context(|| format!("rpc mempool.stats for lane {}", lane.as_str()))?;
    let floor = stats.fee_floor;
    let mut cache = FEE_FLOOR_CACHE.lock().unwrap();
    cache.insert(
        key,
        CacheEntry {
            floor,
            fetched_at: Instant::now(),
        },
    );
    Ok(floor)
}

fn record_wallet_event(
    client: &RpcClient,
    rpc: &str,
    kind: WalletEventKind,
    lane: FeeLane,
    fee: u64,
    floor: u64,
    json: bool,
) {
    let event = WalletQosEvent {
        event: kind.as_str(),
        lane: lane.as_str(),
        fee,
        floor,
    };
    if let Err(err) = client.record_wallet_qos_event(rpc, event) {
        if !json {
            let msg = match &err {
                WalletQosError::Transport(_) => {
                    format!("failed to record wallet telemetry: {err}")
                }
                WalletQosError::Rpc { code, message } => {
                    format!("wallet telemetry rejected by node (code {code}): {message}")
                }
                WalletQosError::MissingStatus => {
                    "wallet telemetry response missing status field".to_string()
                }
                WalletQosError::InvalidStatus(status) => {
                    format!("wallet telemetry response returned status '{status}'")
                }
            };
            eprintln!("{msg}");
        }
    }
}

fn prompt_user(localizer: &Localizer) -> Result<PromptDecision> {
    loop {
        print!("{}", localizer.prompt());
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim().to_lowercase();
        match trimmed.as_str() {
            "a" | "auto" => return Ok(PromptDecision::Auto),
            "f" | "force" => return Ok(PromptDecision::Force),
            "c" | "cancel" | "q" | "quit" => return Ok(PromptDecision::Cancel),
            _ => println!("{}", localizer.invalid_choice()),
        }
    }
}

fn parse_lane(lane: &str) -> Result<FeeLane> {
    match lane.to_ascii_lowercase().as_str() {
        "consumer" => Ok(FeeLane::Consumer),
        "industrial" => Ok(FeeLane::Industrial),
        other => Err(anyhow!(
            "unknown lane '{other}', expected consumer or industrial"
        )),
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Language {
    En,
    Es,
    Fr,
    De,
    Pt,
    Zh,
}

impl Language {
    pub fn detect(explicit: &Option<String>) -> Self {
        if let Some(code) = explicit {
            return Self::from_code(code);
        }
        if let Ok(code) = std::env::var("TB_LANG") {
            if !code.is_empty() {
                return Self::from_code(&code);
            }
        }
        if let Ok(code) = std::env::var("LANG") {
            if !code.is_empty() {
                return Self::from_code(&code);
            }
        }
        Language::En
    }

    pub fn from_code(code: &str) -> Self {
        let lower = code.to_ascii_lowercase();
        if lower.starts_with("es") {
            Language::Es
        } else if lower.starts_with("fr") {
            Language::Fr
        } else if lower.starts_with("de") {
            Language::De
        } else if lower.starts_with("pt") {
            Language::Pt
        } else if lower.starts_with("zh") {
            Language::Zh
        } else {
            Language::En
        }
    }
}

pub struct Localizer {
    lang: Language,
}

impl Localizer {
    pub fn new(lang: Language) -> Self {
        Self { lang }
    }

    fn lane_label(&self, lane: FeeLane) -> &'static str {
        match (self.lang, lane) {
            (Language::Es, FeeLane::Consumer) => "consumo",
            (Language::Es, FeeLane::Industrial) => "industrial",
            (Language::Fr, FeeLane::Consumer) => "consommateur",
            (Language::Fr, FeeLane::Industrial) => "industriel",
            (Language::De, FeeLane::Consumer) => "verbraucher",
            (Language::De, FeeLane::Industrial) => "industrie",
            (Language::Pt, FeeLane::Consumer) => "consumo",
            (Language::Pt, FeeLane::Industrial) => "industrial",
            (Language::Zh, FeeLane::Consumer) => "消费",
            (Language::Zh, FeeLane::Industrial) => "工业",
            (_, FeeLane::Consumer) => "consumer",
            (_, FeeLane::Industrial) => "industrial",
        }
    }

    fn warning_message(&self, lane: FeeLane, fee: u64, floor: u64) -> String {
        match self.lang {
            Language::Es => format!(
                "Advertencia: la tarifa {fee} está por debajo del piso {} ({floor}).",
                self.lane_label(lane)
            ),
            Language::Fr => format!(
                "Avertissement : les frais {fee} sont inférieurs au plancher {} ({floor}).",
                self.lane_label(lane)
            ),
            Language::De => format!(
                "Warnung: Gebühr {fee} liegt unter dem {}-Gebührenboden ({floor}).",
                self.lane_label(lane)
            ),
            Language::Pt => format!(
                "Aviso: a taxa {fee} está abaixo do piso {} ({floor}).",
                self.lane_label(lane)
            ),
            Language::Zh => format!(
                "警告：费用 {fee} 低于 {} 费用下限 ({floor})。",
                self.lane_label(lane)
            ),
            Language::En => format!(
                "Warning: fee {fee} is below the {} fee floor ({floor}).",
                self.lane_label(lane)
            ),
        }
    }

    fn auto_bump_confirmation(&self, floor: u64) -> String {
        match self.lang {
            Language::Es => format!("Ajustando automáticamente la tarifa al piso {floor}."),
            Language::Fr => format!("Ajustement automatique des frais au plancher {floor}."),
            Language::De => format!("Automatisches Anheben der Gebühr auf den Boden {floor}."),
            Language::Pt => format!("Aumentando automaticamente a taxa para o piso {floor}."),
            Language::Zh => format!("自动将费用提升至下限 {floor}。"),
            Language::En => format!("Auto-bumping fee to floor {floor}."),
        }
    }

    fn force_confirmation(&self, fee: u64, floor: u64) -> String {
        match self.lang {
            Language::Es => {
                format!("Forzando el envío con tarifa {fee} por debajo del piso {floor}.")
            }
            Language::Fr => {
                format!("Envoi forcé avec des frais {fee} en dessous du plancher {floor}.")
            }
            Language::De => {
                format!("Senden wird mit Gebühr {fee} unter dem Gebührenboden {floor} erzwungen.")
            }
            Language::Pt => format!("Forçando o envio com taxa {fee} abaixo do piso {floor}."),
            Language::Zh => format!("强制以费用 {fee}（低于下限 {floor}）发送。"),
            Language::En => format!("Forcing send with fee {fee} below floor {floor}."),
        }
    }

    fn cancelled(&self) -> String {
        match self.lang {
            Language::Es => "Operación cancelada.".to_string(),
            Language::Fr => "Opération annulée.".to_string(),
            Language::De => "Vorgang abgebrochen.".to_string(),
            Language::Pt => "Operação cancelada.".to_string(),
            Language::Zh => "操作已取消。".to_string(),
            Language::En => "Transaction cancelled.".to_string(),
        }
    }

    fn needs_confirmation(&self) -> String {
        match self.lang {
            Language::Es => "La tarifa está por debajo del piso. Ejecute de nuevo con --auto-bump o --force para continuar.".to_string(),
            Language::Fr => "Les frais sont sous le plancher. Relancez avec --auto-bump ou --force pour continuer.".to_string(),
            Language::De => "Gebühr unter dem Minimum. Erneut mit --auto-bump oder --force ausführen.".to_string(),
            Language::Pt => "A taxa está abaixo do piso. Reexecute com --auto-bump ou --force para continuar.".to_string(),
            Language::Zh => "费用低于阈值。请使用 --auto-bump 或 --force 重新运行以继续。".to_string(),
            Language::En => "Fee is below the mempool floor; re-run with --auto-bump or --force to continue.".to_string(),
        }
    }

    fn prompt(&self) -> String {
        match self.lang {
            Language::Es => "Elegir [a]uto, [f]orzar o [c]ancelar: ".to_string(),
            Language::Fr => "Choisir [a]uto, [f]orcer ou [c]annuler : ".to_string(),
            Language::De => "[a]utomatisch, [f]erzwingen oder [c]abbrechen wählen: ".to_string(),
            Language::Pt => "Escolha [a]uto, [f]orçar ou [c]ancelar: ".to_string(),
            Language::Zh => "选择 [a] 自动调整、[f] 强制或 [c] 取消：".to_string(),
            Language::En => "Choose [a]uto bump, [f]orce, or [c]ancel: ".to_string(),
        }
    }

    fn invalid_choice(&self) -> String {
        match self.lang {
            Language::Es => "Opción no válida, inténtelo de nuevo.".to_string(),
            Language::Fr => "Choix invalide, veuillez réessayer.".to_string(),
            Language::De => "Ungültige Auswahl, bitte erneut versuchen.".to_string(),
            Language::Pt => "Opção inválida, tente novamente.".to_string(),
            Language::Zh => "无效选项，请重试。".to_string(),
            Language::En => "Unrecognised option, please try again.".to_string(),
        }
    }

    fn success_message(
        &self,
        from: &str,
        to: &str,
        amount: u64,
        fee: u64,
        floor: u64,
        lane: FeeLane,
        auto_bumped: bool,
        forced: bool,
    ) -> String {
        let lane_label = self.lane_label(lane);
        let adjustment = if forced {
            match self.lang {
                Language::Es => " (forzado por debajo del piso)".to_string(),
                Language::Fr => " (forcé sous le plancher)".to_string(),
                Language::De => " (erzwungen unter dem Minimum)".to_string(),
                Language::Pt => " (forçado abaixo do piso)".to_string(),
                Language::Zh => "（低于下限强制发送）".to_string(),
                Language::En => " (forced below floor)".to_string(),
            }
        } else if auto_bumped {
            match self.lang {
                Language::Es => " (ajuste automático)".to_string(),
                Language::Fr => " (ajustement automatique)".to_string(),
                Language::De => " (automatisch angepasst)".to_string(),
                Language::Pt => " (ajuste automático)".to_string(),
                Language::Zh => "（已自动调整）".to_string(),
                Language::En => " (auto-bumped)".to_string(),
            }
        } else {
            String::new()
        };
        match self.lang {
            Language::Es => format!(
                "Transacción preparada de {from} a {to} por {amount} en la vía {lane_label} con tarifa {fee} (piso {floor}){adjustment}."
            ),
            Language::Fr => format!(
                "Transaction préparée de {from} vers {to} pour {amount} sur la voie {lane_label} avec des frais {fee} (plancher {floor}){adjustment}."
            ),
            Language::De => format!(
                "Transaktion von {from} an {to} über {amount} im Kanal {lane_label} mit Gebühr {fee} (Grenze {floor}){adjustment} vorbereitet."
            ),
            Language::Pt => format!(
                "Transação preparada de {from} para {to} por {amount} na fila {lane_label} com taxa {fee} (piso {floor}){adjustment}."
            ),
            Language::Zh => format!(
                "已为 {from} 向 {to} 准备金额 {amount} 的 {lane_label} 交易，费用 {fee}（下限 {floor}）{adjustment}。"
            ),
            Language::En => format!(
                "Transaction prepared from {from} to {to} for {amount} on the {lane_label} lane at fee {fee} (floor {floor}){adjustment}."
            ),
        }
    }

    fn ephemeral_notice(&self, addr: &str, amount: u64, to: &str) -> String {
        match self.lang {
            Language::Es => {
                format!("Se usa la dirección efímera {addr} para transferir {amount} a {to}")
            }
            Language::Fr => {
                format!("Adresse éphémère {addr} utilisée pour transférer {amount} à {to}")
            }
            Language::De => format!(
                "Ephemere Adresse {addr} wird für die Überweisung von {amount} an {to} verwendet"
            ),
            Language::Pt => {
                format!("Endereço efêmero {addr} usado para transferir {amount} para {to}")
            }
            Language::Zh => format!("使用临时地址 {addr} 向 {to} 转账 {amount}"),
            Language::En => {
                format!("ephemeral address {addr} used for transfer of {amount} to {to}")
            }
        }
    }
}
