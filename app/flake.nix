{
  description = "Provenance: the Basecamp panel that browses and resolves the LEZ v0.3 program registry";

  nixConfig = {
    extra-substituters = [ "https://cache.nix.logos.co/public" ];
    extra-trusted-public-keys = [
      "public:l4HrXgL4nw246+LBh2SOJyhz64BoGegOYLheT/iIAPU="
    ];
  };

  # The builder revision the Basecamp 0.3.0 catalog modules are built with. Its
  # Qt is 6.9.2, the version Basecamp 0.3.0 bundles; Qt refuses a plugin built
  # against a newer minor than the host's, so this pin is a ceiling as well as
  # a toolchain.
  inputs = {
    logos-module-builder.url = "github:logos-co/logos-module-builder/fb8d5513ed2e6ce34d998db59e425d0cbeb985e3";
  };

  outputs = inputs@{ logos-module-builder, ... }:
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
    };
}
