{ self }:
{ config, lib, pkgs, ... }:
let
  inherit (lib)
    concatMap
    escapeShellArgs
    mkEnableOption
    mkIf
    mkOption
    optionals
    types
    ;

  packagesForSystem = lib.attrByPath [ pkgs.system ] (self.packages or { }) { };

  defaultPackage =
    packagesForSystem.intercept-bounce
    or packagesForSystem.default
    or (throw "intercept-bounce package not available for system ${pkgs.system}");

  cfg = config.services.interceptBounce;

  toStr = value:
    if builtins.isString value then value else builtins.toString value;

  baseArgs =
    []
    ++ optionals (cfg.debounceTime != null) [ "--debounce-time" cfg.debounceTime ]
    ++ optionals (cfg.nearMissThresholdTime != null) [
      "--near-miss-threshold-time"
      cfg.nearMissThresholdTime
    ]
    ++ optionals (cfg.logInterval != null) [ "--log-interval" cfg.logInterval ]
    ++ optionals cfg.logAllEvents [ "--log-all-events" ]
    ++ optionals cfg.logBounces [ "--log-bounces" ]
    ++ optionals cfg.listDevices [ "--list-devices" ]
    ++ optionals cfg.statsJson [ "--stats-json" ]
    ++ optionals cfg.verbose [ "--verbose" ]
    ++ optionals (cfg.ringBufferSize != null) [
      "--ring-buffer-size"
      toStr cfg.ringBufferSize
    ]
    ++ concatMap (key: [ "--debounce-key" key ]) cfg.debounceKeys
    ++ concatMap (key: [ "--ignore-key" key ]) cfg.ignoreKeys
    ++ optionals (cfg.otelEndpoint != null) [
      "--otel-endpoint"
      cfg.otelEndpoint
    ]
    ++ cfg.extraArgs;

  commandList = [ "${cfg.package}/bin/intercept-bounce" ] ++ baseArgs;
  commandString = escapeShellArgs commandList;
in
{
  options.services.interceptBounce = {
    enable = mkEnableOption "intercept-bounce CLI integration";

    package = mkOption {
      type = types.package;
      default = defaultPackage;
      description = "Package providing the intercept-bounce executable.";
    };

    installSystemPackage = mkOption {
      type = types.bool;
      default = true;
      description = "Add intercept-bounce to environment.systemPackages when enabled.";
    };

    debounceTime = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "20ms";
      description = "Window passed to --debounce-time.";
    };

    nearMissThresholdTime = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "100ms";
      description = "Window passed to --near-miss-threshold-time.";
    };

    logInterval = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "15m";
      description = "Period passed to --log-interval.";
    };

    logAllEvents = mkOption {
      type = types.bool;
      default = false;
      description = "Enable --log-all-events.";
    };

    logBounces = mkOption {
      type = types.bool;
      default = false;
      description = "Enable --log-bounces.";
    };

    listDevices = mkOption {
      type = types.bool;
      default = false;
      description = "Enable --list-devices.";
    };

    statsJson = mkOption {
      type = types.bool;
      default = false;
      description = "Enable --stats-json.";
    };

    verbose = mkOption {
      type = types.bool;
      default = false;
      description = "Enable --verbose.";
    };

    ringBufferSize = mkOption {
      type = types.nullOr types.ints.unsigned;
      default = null;
      description = "Size passed to --ring-buffer-size.";
    };

    debounceKeys = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "List of keys passed via repeated --debounce-key flags.";
    };

    ignoreKeys = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "List of keys passed via repeated --ignore-key flags.";
    };

    otelEndpoint = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Endpoint supplied to --otel-endpoint.";
    };

    extraArgs = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Arbitrary arguments appended after generated flags.";
    };

    command = mkOption {
      type = types.listOf types.str;
      default = [ ];
      description = "Resolved intercept-bounce invocation expressed as a list.";
    };

    commandString = mkOption {
      type = types.str;
      default = "";
      description = "Resolved intercept-bounce invocation rendered for shell pipelines.";
    };
  };

  config = mkIf cfg.enable {
    environment.systemPackages = optionals cfg.installSystemPackage [ cfg.package ];
    services.interceptBounce.command = commandList;
    services.interceptBounce.commandString = commandString;
  };
}
