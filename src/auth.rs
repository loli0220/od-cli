use crate::config::Config;
use crate::types::UserProfile;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use colored::Colorize;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

pub const OAUTH_SCOPES: &str = "Files.ReadWrite.All offline_access User.Read";

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: Option<u64>,
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TokenResponse {
    pub token_type: String,
    pub scope: Option<String>,
    pub expires_in: i64,
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
    error_description: Option<String>,
}

pub struct AuthManager {
    http: Client,
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn login(&self, config: &mut Config) -> Result<()> {
        let client_id = config.get_client_id();
        let tenant_id = config.get_tenant_id();

        println!("{}", "==> Initiating Microsoft Azure Device Login...".cyan().bold());

        let device_url = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/devicecode",
            tenant_id
        );

        let mut params = HashMap::new();
        params.insert("client_id", client_id.as_str());
        params.insert("scope", OAUTH_SCOPES);

        let res = self
            .http
            .post(&device_url)
            .form(&params)
            .send()
            .await
            .context("Failed to connect to Azure device code endpoint")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            bail!(
                "Failed to request device authorization code: {}\nMake sure the Client ID and Tenant ID are correct in Azure App Registration.",
                error_text
            );
        }

        let device_res: DeviceCodeResponse = res
            .json()
            .await
            .context("Failed to parse device code response")?;

        println!();
        println!("{}", "=================== Microsoft Sign-In ===================".yellow().bold());
        println!("1. Open the URL in your browser: {}", device_res.verification_uri.bright_cyan().underline());
        println!("2. Enter verification code:      {}", device_res.user_code.bright_green().bold());
        println!("{}", "=========================================================".yellow().bold());
        println!();

        // Try opening browser automatically
        let _ = open::that(&device_res.verification_uri);

        let poll_interval = device_res.interval.unwrap_or(5).max(3);
        let expires_in = device_res.expires_in;
        let start_time = Utc::now().timestamp();

        let token_url = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            tenant_id
        );

        let spinner = indicatif::ProgressBar::new_spinner();
        spinner.set_message("Waiting for sign-in in browser...");
        spinner.enable_steady_tick(Duration::from_millis(120));

        let token_result: Result<TokenResponse> = async {
            loop {
                if Utc::now().timestamp() - start_time >= expires_in {
                    bail!("Authentication code expired. Please try logging in again.");
                }

                sleep(Duration::from_secs(poll_interval)).await;

                let mut token_params = HashMap::new();
                token_params.insert("grant_type", "urn:ietf:params:oauth:grant-type:device_code");
                token_params.insert("client_id", client_id.as_str());
                token_params.insert("device_code", device_res.device_code.as_str());

                let token_res = self
                    .http
                    .post(&token_url)
                    .form(&token_params)
                    .send()
                    .await?;

                if token_res.status().is_success() {
                    let token: TokenResponse = token_res.json().await?;
                    return Ok(token);
                }

                let status = token_res.status();
                if let Ok(err_data) = token_res.json::<ErrorResponse>().await {
                    match err_data.error.as_str() {
                        "authorization_pending" => {
                            // Still waiting for user confirmation
                            continue;
                        }
                        "slow_down" => {
                            sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                        "expired_token" | "code_expired" => {
                            bail!("Sign-in session timed out.");
                        }
                        "access_denied" => {
                            bail!("Sign-in authorization was declined.");
                        }
                        other => {
                            let desc = err_data.error_description.unwrap_or_default();
                            if desc.contains("AADSTS65002") {
                                bail!(
                                    "Azure AD rejected the Client ID (AADSTS65002: first-party app consent restricted).\n\n\
                                    💡 Solution: Please register your own free Azure App in Azure Portal:\n\
                                    1. Open https://portal.azure.com/#blade/Microsoft_AAD_RegisteredApps/ApplicationsListBlade\n\
                                    2. Click 'New registration', name it 'od-cli', select 'Accounts in any organizational directory and personal Microsoft accounts'\n\
                                    3. In Authentication -> Advanced settings -> set 'Allow public client flows' to 'Yes'\n\
                                    4. In API permissions -> Add 'Files.ReadWrite.All', 'offline_access', 'User.Read' (Delegated permissions)\n\
                                    5. Run `od-cli config set client_id <YOUR_CLIENT_ID>` and then `od-cli auth login`"
                                );
                            } else {
                                bail!("Authentication failed ({}): {}", other, desc);
                            }
                        }
                    }
                } else {
                    bail!("Unexpected error during authentication (HTTP status: {})", status);
                }
            }
        }
        .await;

        match token_result {
            Ok(tokens) => {
                spinner.finish_with_message("Sign-in authorized successfully!");
                let now = Utc::now().timestamp();
                config.access_token = Some(tokens.access_token);
                if let Some(rt) = tokens.refresh_token {
                    config.refresh_token = Some(rt);
                }
                config.expires_at = Some(now + tokens.expires_in);

                // Fetch user profile info
                if let Ok(profile) = self.fetch_user_profile(config).await {
                    config.user_principal_name = profile.user_principal_name.or(profile.mail);
                    config.display_name = profile.display_name;
                }

                config.save()?;
                println!();
                println!("{}", " Authentication Successful!".bright_green().bold());
                if let Some(ref email) = config.user_principal_name {
                    println!("Logged in as: {}", email.bright_yellow());
                }
                Ok(())
            }
            Err(e) => {
                spinner.finish_with_message("Sign-in failed.");
                Err(e)
            }
        }
    }

    pub async fn ensure_valid_token(&self, config: &mut Config) -> Result<String> {
        if config.access_token.is_none() {
            bail!("Not logged in. Please run `od-cli auth login` first.");
        }

        if !config.is_token_expired() {
            return Ok(config.access_token.clone().unwrap());
        }

        // Token expired, attempt refresh
        let refresh_token = match &config.refresh_token {
            Some(rt) if !rt.trim().is_empty() => rt.clone(),
            _ => {
                bail!("Access token expired and no refresh token found. Please run `od-cli auth login` again.");
            }
        };

        let client_id = config.get_client_id();
        let tenant_id = config.get_tenant_id();
        let token_url = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            tenant_id
        );

        let mut params = HashMap::new();
        params.insert("grant_type", "refresh_token");
        params.insert("client_id", client_id.as_str());
        params.insert("refresh_token", refresh_token.as_str());
        params.insert("scope", OAUTH_SCOPES);

        let res = self
            .http
            .post(&token_url)
            .form(&params)
            .send()
            .await
            .context("Failed to connect to Azure token endpoint for refresh")?;

        if !res.status().is_success() {
            let error_text = res.text().await.unwrap_or_default();
            bail!(
                "Failed to refresh access token: {}. Please run `od-cli auth login` to re-authenticate.",
                error_text
            );
        }

        let tokens: TokenResponse = res
            .json()
            .await
            .context("Failed to parse refresh token response")?;

        let now = Utc::now().timestamp();
        config.access_token = Some(tokens.access_token.clone());
        if let Some(new_rt) = tokens.refresh_token {
            config.refresh_token = Some(new_rt);
        }
        config.expires_at = Some(now + tokens.expires_in);
        config.save()?;

        Ok(tokens.access_token)
    }

    pub async fn fetch_user_profile(&self, config: &Config) -> Result<UserProfile> {
        let token = config
            .access_token
            .as_ref()
            .context("No access token available")?;

        let res = self
            .http
            .get("https://graph.microsoft.com/v1.0/me")
            .bearer_auth(token)
            .send()
            .await
            .context("Failed to fetch user profile")?;

        if res.status().is_success() {
            let profile: UserProfile = res.json().await?;
            Ok(profile)
        } else {
            bail!("Failed to get user profile: HTTP {}", res.status());
        }
    }
}
