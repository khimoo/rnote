{
  lib,
  stdenv,
  src,
  alsa-lib,
  appstream,
  appstream-glib,
  cargo,
  cmake,
  desktop-file-utils,
  dos2unix,
  glib,
  gst_all_1,
  gtk4,
  libadwaita,
  libxml2,
  meson,
  ninja,
  pkg-config,
  python3,
  rustPlatform,
  rustc,
  shared-mime-info,
  wrapGAppsHook4,
}:

stdenv.mkDerivation (finalAttrs: {
  pname = "rnote";
  version = (lib.importTOML ../Cargo.toml).workspace.package.version;
  inherit src;

  cargoDeps = rustPlatform.fetchCargoVendor {
    inherit (finalAttrs) pname version src;
    hash = lib.fakeHash;
  };

  nativeBuildInputs = [
    appstream-glib
    cmake
    desktop-file-utils
    dos2unix
    meson
    ninja
    pkg-config
    python3
    rustPlatform.bindgenHook
    rustPlatform.cargoSetupHook
    cargo
    rustc
    shared-mime-info
    wrapGAppsHook4
  ];

  dontUseCmakeConfigure = true;

  mesonFlags = [ (lib.mesonBool "cli" true) ];

  buildInputs = [
    appstream
    glib
    gst_all_1.gstreamer
    gtk4
    libadwaita
    libxml2
  ]
  ++ lib.optionals stdenv.hostPlatform.isLinux [ alsa-lib ];

  postPatch = ''
    chmod +x build-aux/*.py
    patchShebangs build-aux
  '';

  postInstall = ''
    substituteInPlace $out/share/thumbnailers/rnote.thumbnailer \
      --replace-fail "TryExec=rnote-cli" "TryExec=$out/bin/rnote-cli" \
      --replace-fail "Exec=rnote-cli" "Exec=$out/bin/rnote-cli"
  '';

  meta = {
    description = "Rnote with PDF page references (personal fork of flxzt/rnote)";
    homepage = "https://github.com/khimoo/rnote";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "rnote";
  };
})
