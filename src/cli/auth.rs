use clap::{Args, Subcommand};
use serde_json::{Value, json};

use super::GlobalArgs;
use crate::client::{Client, Error, Request, Result};
use crate::config::{Config, Credentials, Profile};
use crate::output::{Output, Printer, View};

#[derive(Debug, Args)]
pub struct AuthCmd {
    #[command(subcommand)]
    command: AuthSub,
}

#[derive(Debug, Subcommand)]
enum AuthSub {
    /// Save an API key so later commands need no environment setup.
    ///
    /// The key is written to ~/.config/pf/config.toml with permissions that
    /// keep it readable only by you.
    Login(LoginArgs),

    /// Forget a saved API key.
    Logout,

    /// Show which key is in use, where it came from, and whether it works.
    Status,

    /// Print the resolved token, for passing to other tools.
    Token,
}

#[derive(Debug, Args)]
struct LoginArgs {
    /// Save the key without checking it against the API first.
    #[arg(long)]
    no_verify: bool,

    /// Make this profile the one used when --profile is not given.
    #[arg(long)]
    set_default: bool,
}

impl AuthCmd {
    /// `auth` manages the credentials every other command consumes, so it takes
    /// the raw arguments rather than a ready-made client: `login` has to run
    /// when no usable token exists yet.
    pub async fn run(
        &self,
        global: &GlobalArgs,
        config: Config,
        printer: &Printer,
    ) -> Result<()> {
        match &self.command {
            AuthSub::Login(args) => login(global, config, args, printer).await,
            AuthSub::Logout => logout(global, config, printer),
            AuthSub::Status => status(global, &config, printer).await,
            AuthSub::Token => {
                let credentials = global.credentials(&config)?;
                println!("{}", credentials.token);
                Ok(())
            }
        }
    }
}

async fn login(
    global: &GlobalArgs,
    mut config: Config,
    args: &LoginArgs,
    printer: &Printer,
) -> Result<()> {
    // Deliberately not the environment: logging in should save the key the user
    // just supplied, not quietly persist one that happened to be exported.
    let token = if let Some(token) = &global.token {
        token.trim().to_string()
    } else if let Some(path) = &global.token_file {
        crate::config::read_token_file(path)?
    } else {
        crate::config::prompt_for_token()?
    };

    let base_url = global
        .base_url
        .clone()
        .or_else(|| std::env::var(crate::config::BASE_URL_ENV).ok().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| crate::client::DEFAULT_BASE_URL.to_string());

    let credentials =
        Credentials { token: token.clone(), source: "login".into(), base_url: base_url.clone() };

    if !args.no_verify {
        let client = Client::new(
            credentials.clone(),
            global.timeout,
            global.max_retries,
            global.dry_run,
            global.verbose,
        )?;
        // Saving a key that does not work just moves the failure to the next
        // command, where it is harder to explain.
        client.send(Request::get("/labels")).await.map_err(|mut e| {
            if e.status == Some(401) || e.status == Some(403) {
                e.message = "That key was rejected by the API; nothing was saved.".into();
                e.hint = Some(
                    "Check you copied the whole key, and that it has not expired or been \
                     revoked in Settings > API Keys."
                        .into(),
                );
            }
            e
        })?;
    }

    let custom_base_url =
        (base_url != crate::client::DEFAULT_BASE_URL).then(|| base_url.clone());

    let location = match config.active_profile_name(global.profile.as_deref()) {
        Some(name) => {
            config
                .profiles
                .insert(name.clone(), Profile { token, base_url: custom_base_url });
            // The first profile saved becomes the default, so a single-profile
            // setup never has to pass --profile.
            if args.set_default || config.default_profile.is_none() && config.profiles.len() == 1 {
                config.default_profile = Some(name.clone());
            }
            format!("profile `{name}`")
        }
        None => {
            config.token = Some(token);
            config.base_url = custom_base_url;
            "the default credentials".to_string()
        }
    };

    let path = global.config_path().ok_or_else(|| {
        Error::other("Could not work out where to write the config.")
            .with_hint("Set PF_CONFIG to an explicit path.")
    })?;
    config.save(&path)?;

    let value = json!({
        "data": {
            "saved": true,
            "configPath": path.display().to_string(),
            "profile": config.active_profile_name(global.profile.as_deref()),
            "baseUrl": base_url,
            "verified": !args.no_verify,
        }
    });

    printer.emit(&Output::new(value, View::Silent).note(format!(
        "\u{2713} Saved {location} to {}{}",
        path.display(),
        if args.no_verify { " (unverified)" } else { "" }
    )));
    Ok(())
}

fn logout(global: &GlobalArgs, mut config: Config, printer: &Printer) -> Result<()> {
    let path = global
        .config_path()
        .ok_or_else(|| Error::other("Could not work out where the config lives."))?;

    let removed = match config.active_profile_name(global.profile.as_deref()) {
        Some(name) => {
            if config.profiles.remove(&name).is_none() {
                return Err(Error::usage(format!("No profile named `{name}` is saved.")));
            }
            if config.default_profile.as_deref() == Some(name.as_str()) {
                // Leave the default pointing at something that exists.
                config.default_profile = config.profiles.keys().next().cloned();
            }
            format!("profile `{name}`")
        }
        None => {
            if config.token.take().is_none() {
                return Err(Error::usage("No saved credentials to remove."));
            }
            config.base_url = None;
            "the default credentials".to_string()
        }
    };

    config.save(&path)?;

    let value = json!({ "data": { "removed": true, "configPath": path.display().to_string() } });
    printer.emit(&Output::new(value, View::Silent).note(format!(
        "\u{2713} Removed {removed} from {}",
        path.display()
    )));
    Ok(())
}

/// There is no `/me` endpoint, so the cheapest honest probe is the smallest
/// unpaginated read the API offers.
async fn status(global: &GlobalArgs, config: &Config, printer: &Printer) -> Result<()> {
    let credentials = global.credentials(config)?;
    let client = Client::new(
        credentials.clone(),
        global.timeout,
        global.max_retries,
        global.dry_run,
        global.verbose,
    )?;

    let response = client.send(Request::get("/labels")).await?;

    if client.is_dry_run() {
        printer.emit_dry_run(&client.take_dry_run_log());
        return Ok(());
    }

    let labels = response.get("data").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    let profile = config.active_profile_name(global.profile.as_deref());
    let config_path = global.config_path().map(|p| p.display().to_string());

    let value = json!({
        "data": {
            "ok": true,
            "baseUrl": client.base_url(),
            "tokenSource": client.token_source(),
            "token": client.masked_token(),
            "profile": profile,
            "configPath": config_path,
            "labelCount": labels,
        }
    });

    let profile_line = profile
        .as_deref()
        .map(|p| format!("\n  profile {p}"))
        .unwrap_or_default();

    printer.emit(&Output::new(value, View::Silent).note(format!(
        "\u{2713} {}\n  token {} (from {}){profile_line}\n  {labels} label{} readable",
        client.base_url(),
        client.masked_token(),
        client.token_source(),
        if labels == 1 { "" } else { "s" },
    )));
    Ok(())
}
