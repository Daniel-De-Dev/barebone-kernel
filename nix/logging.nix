let
  subsystemFeatures = [
    "log-frames"
    "log-boot"
    "log-paging"
  ];

  levelFeatures = {
    info = [ ];
    debug = [ "log-debug" ];
    trace = [ "log-trace" ];
  };

  variants = {
    "" = {
      release = true;
      level = "info";
    };
    "-debug" = {
      release = false;
      level = "debug";
    };
    "-trace" = {
      release = false;
      level = "trace";
    };
  };
in
builtins.mapAttrs (
  _: variant:
  variant
  // {
    loggingFeatures =
      levelFeatures.${variant.level}
      ++ (if variant.level == "info" then [ ] else subsystemFeatures);
  }
) variants
