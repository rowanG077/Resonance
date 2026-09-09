{
  lib,
  stdenv,
  hvqm4Src,
}:
stdenv.mkDerivation {
  pname = "resonance-hvqm4-video";
  version = "2020-11-07";
  src = hvqm4Src;
  patches = [ ./hvqm4-raw-output.patch ];
  dontConfigure = true;
  buildPhase = ''
    runHook preBuild
    $CC -O2 -DNATIVE=1 h4m_audio_decode.c -o hvqm4-video
    runHook postBuild
  '';
  installPhase = ''
    mkdir -p $out/bin $out/share/doc/hvqm4-video
    cp hvqm4-video $out/bin/
    cp README.md $out/share/doc/hvqm4-video/
  '';
  meta = {
    description = "Offline HVQM4 decoder with planar video output";
    homepage = "https://github.com/Tilka/hvqm4";
    license = lib.licenses.lgpl2Plus;
    platforms = lib.platforms.unix;
  };
}
