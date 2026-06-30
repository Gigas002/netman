// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2024 netman contributors
//
// netman — TUI applet analogue of network-manager-applet

use anyhow::Result;
use netman::{app, cli, config, logger, settings};

#[tokio::main]
async fn main() -> Result<()> {
    let cli_opts = cli::parse();
    let file_cfg = config::load(cli_opts.config.as_deref())?;
    let resolved = settings::resolve(cli_opts, file_cfg);
    logger::init(&resolved)?;
    app::run(resolved).await
}
