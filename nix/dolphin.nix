{ pkgs }:
pkgs.dolphin-emu.overrideAttrs (previous: {
  patches = (previous.patches or [ ]) ++ [ ./dolphin-mem1-observer.patch ];
})
