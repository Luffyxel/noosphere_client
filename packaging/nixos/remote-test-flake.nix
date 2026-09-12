{
  description = "Noosphere Remote Linux test environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in {
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          cargo
          git
          pkg-config
          protobuf
          rustc
          xorg.xorgserver
        ];
        buildInputs = with pkgs; [
          gst_all_1.gstreamer
          gst_all_1.gst-libav
          gst_all_1.gst-plugins-base
          gst_all_1.gst-plugins-good
          gst_all_1.gst-plugins-bad
          gst_all_1.gst-plugins-ugly
          libxkbcommon
          openssl
          pipewire
          wayland
        ];
        NOOSPHERE_REMOTE_TEST_SOURCE = "1";
        NOOSPHERE_REMOTE_TEST_SINK = "1";
      };
    };
}
