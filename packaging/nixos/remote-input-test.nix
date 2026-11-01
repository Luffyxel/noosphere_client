{ pkgs ? import <nixpkgs> { } }:
let
  probe = builtins.path {
    path = ../../src-tauri/target/debug/noosphere-remote-bench;
    name = "noosphere-remote-bench";
  };
in
pkgs.testers.runNixOSTest {
  name = "noosphere-hyprland-input";

  nodes.machine = { pkgs, ... }: {
    imports = [ ./remote-test.nix ];
    xdg.portal.enable = pkgs.lib.mkForce false;
    services.speechd.enable = false;
    services.getty.autologinUser = "alice";
    users.users.alice.isNormalUser = true;
    programs.nix-ld = {
      enable = true;
      libraries = with pkgs; [
        stdenv.cc.cc.lib
        glib
        libxkbcommon
        openssl
        wayland
        gst_all_1.gstreamer
        gst_all_1.gst-plugins-base
      ];
    };
    environment.systemPackages = with pkgs; [ jq wev ];
    environment.etc."noosphere-input.conf".text = ''
      monitor = Noosphere-1,1920x1080@60,0x0,1
      monitor = Noosphere-2,1920x1080@60,1920x0,1
      monitor = ,disable
      animations {
        enabled = false
      }
      misc {
        disable_hyprland_logo = true
        disable_splash_rendering = true
      }
      bind = ,mouse:272,exec,touch /tmp/noosphere-clicked
      bind = ,TAB,exec,touch /tmp/noosphere-keyboard
    '';
    system.stateVersion = "26.05";
    virtualisation = {
      cores = 2;
      memorySize = 4096;
      resolution = { x = 1920; y = 1080; };
      qemu.options = [ "-vga none -device virtio-gpu-pci" ];
    };
  };

  testScript = ''
    import json

    start_all()
    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("user@1000.service")
    machine.succeed("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 HYPRLAND_HEADLESS_ONLY=1 Hyprland --config /etc/noosphere-input.conf > /tmp/noosphere-hyprland.log 2>&1 &' ")
    try:
        machine.wait_until_succeeds("test -n \"$(find /run/user/1000/hypr -name .socket.sock -print -quit)\"", timeout=60)
    except Exception:
        print(machine.execute("cat /tmp/noosphere-hyprland.log")[1])
        raise
    instance = json.loads(machine.succeed("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 hyprctl instances -j'"))[0]
    signature = instance["instance"]
    display = instance["wl_socket"]

    def user(command):
        import shlex
        env = f"XDG_RUNTIME_DIR=/run/user/1000 HYPRLAND_INSTANCE_SIGNATURE={signature} WAYLAND_DISPLAY={display} "
        return machine.succeed("su - alice -c " + shlex.quote(env + command))

    try:
        user("hyprctl output create headless Noosphere-1")
        user("hyprctl output create headless Noosphere-2")
        print(user("hyprctl monitors -j"))
        user("hyprctl dispatch movecursor 10 10")
        print(user("${probe} --linux-pointer-click 16384 32768"))
        position = json.loads(user("hyprctl cursorpos -j"))
        assert abs(position["x"] - 960) <= 2, position
        assert abs(position["y"] - 540) <= 2, position
        machine.wait_until_succeeds("test -f /tmp/noosphere-clicked", timeout=10)
        user("rm /tmp/noosphere-clicked")
        print(user("${probe} --linux-pointer-click 32768 32768 Noosphere-2"))
        position = json.loads(user("hyprctl cursorpos -j"))
        assert abs(position["x"] - 2880) <= 2, position
        assert abs(position["y"] - 540) <= 2, position
        machine.wait_until_succeeds("test -f /tmp/noosphere-clicked", timeout=10)
        user('hyprctl keyword unbind ",mouse:272"')
        user("hyprctl dispatch focusmonitor Noosphere-2")
        user("stdbuf -oL wev > /tmp/noosphere-wev.log 2>&1 &")
        machine.wait_until_succeeds("grep -q xdg_toplevel /tmp/noosphere-wev.log", timeout=10)
        user("hyprctl dispatch fullscreen 0")
        user("hyprctl dispatch movecursor 1921 1")
        print(user("${probe} --linux-pointer-click 32768 32768 Noosphere-2"))
        machine.wait_until_succeeds("grep -q button /tmp/noosphere-wev.log", timeout=10)
        events = machine.succeed("cat /tmp/noosphere-wev.log")
        import re
        pointer_events = [line for line in events.splitlines() if "wl_pointer]" in line]
        motion = next(i for i, line in enumerate(pointer_events) if re.search(r"motion:.*x, y:\s*960\.", line))
        button = next(i for i in range(motion + 1, len(pointer_events)) if "button:" in pointer_events[i])
        assert any(line.endswith("frame") for line in pointer_events[motion + 1:button]), events
        assert re.search(r"button:\s*272", events), events
        assert re.search(r"state:\s*1", events), events
        assert re.search(r"state:\s*0", events), events
        print(user("${probe} --linux-activate"))
        machine.wait_until_succeeds("test -f /tmp/noosphere-keyboard", timeout=10)
        machine.succeed("printf success > /tmp/input-result")
    finally:
        machine.execute("cat /tmp/noosphere-hyprland.log")
        machine.copy_from_vm("/tmp/noosphere-hyprland.log")
        if machine.execute("test -f /tmp/noosphere-wev.log")[0] == 0:
            print(machine.succeed("cat /tmp/noosphere-wev.log"))
            machine.copy_from_vm("/tmp/noosphere-wev.log")
        machine.execute("cp /run/user/1000/hypr/*/hyprland.log /tmp/compositor.log || true")
        if machine.execute("test -f /tmp/compositor.log")[0] == 0:
            machine.copy_from_vm("/tmp/compositor.log")
        if machine.execute("test -f /tmp/input-result")[0] == 0:
            machine.copy_from_vm("/tmp/input-result")
'';
}
