# The background is routed by capability, never by painter

Branch: `hve2-visual-rewrite` — delivery: work-unit commits on the feature branch.
Test runner: `cargo test`. Related contracts: `openspec/specs/capability-routing/spec.md`
(backends are siblings, the core routes capabilities) and
`openspec/specs/wallpaper-delegation/spec.md` (daemon agnostic, delegation never
fails the apply).

Today the router picks a backend by the TYPE of the source (`ExactPath` → the skwd
engine, `SavedManifest` → the mpvpaper plugin) and, with no saved manifest, calls
`mpvpaper::clear_all()` — so applying a theme stops the video wallpaper the user
started from Noctalia's own plugin. The keeper wants the opposite: HVE hands the
theme's background out and whoever is active and able paints it; nobody is married.

## Specs

S1 (keeper, verbatim): "si yo luego después de eso aplico un tema sin fondo animado
siempre gana hve hasta que yo decida manualmente que no es así." Applying a theme
ALWAYS imposes that theme's background: with an animated background it is that video,
painted by whichever available painter can take it; without one it is the theme's
static background, and any video covering it is stopped. Switching one painter off
and the other on keeps working in both directions (the keeper's original
"si desconectas uno y conectas el otro todo funcione correctamente").

S1b (keeper, verbatim): "si después de aplicar el tema yo quiero cambiar el fondo por
que me sale de la nariz o de los huevos... que no haya nada que me lo impida." After
the apply, nothing in HVE impedes or undoes a manual background change; the next theme
apply wins again.

S2 (keeper, verbatim): "HVE no se casa con nadie... tiene que ser agnostico y asi es
como tiene que ser su arquitectura." The routing decision reads DECLARED
CAPABILITIES and LIVE AVAILABILITY only; no backend identity, name or special case
may appear in the decision.

S3 (keeper, verbatim): "podria ser que hve mande el wallpaper de video y que lo
recoja quien este activado en ese momento independientemente de quien este al mando
de pintarlo en pantalla." The theme's animated background is offered as a
capability to whichever available painter can take it.

S4 (keeper, verbatim): "si no hay que sea hve quien mande poner un fondo estatico
del mismo video... y que lo ponga quien este al mando de ponerlo en ese momento."
When no available painter can paint animated, HVE hands the STATIC fallback of that
same animated background (the theme's own poster) to whichever available painter
can take a still.

S5 (keeper, verbatim): "despues del arreglo todos tendran url por que no se ocupan
ellos de descargar si no que descarga HVE y los demas pintan lo que les manda HVE."
Resolving a theme's remote video into a local file is HVE's own step, not a painter
capability: every painter is handed a local path, and no painter downloads.

S6 (keeper, verbatim): "si tenemos que hacer un adaptador para el plugin de
mpvpaper... se crea para que la logica de mpvpaper funcione con nuestro motor
agnostico." Each painter sits behind an adapter; painter-specific logic never leaks
into the router.

S7: When no available painter can paint either the animated background or its
static fallback, the apply does not fail and does not blank the background: it
warns and leaves the background as it was.

S8 (acceptance, keeper's scenario): with skwd-wall off and a theme whose video is a
local file, applying the theme shows that video; with the video painter off,
applying the theme shows its poster.

S9 (keeper, verbatim, 25-oct-2026): "todo lo que aplique HVE manda HVE quiero decir
que si el tema que esta puesto tiene esa paleta se aplica esa paleta... si luego tu
por lo que quieras cambias la paleta eso ya es indiferente... la puedes cambiar a
mano, con noctalia, con skwd-wall, con lo que sea... pero siempre que apliques un
tema ya creado la paleta y los bordes los manda HVE." So applying a theme imposes
the theme's palette and borders whatever painter owns the background, and a later
manual change by any tool is left alone: HVE never re-imposes the theme's palette
until the next theme apply.

## Tasks

- T1 (inline) this document. Done.
- T2 (inline) capability contract: what a painter can paint, whether it is
  available, and the pure routing decision (animated first, then its static
  fallback, exclusive turn, nobody-able path). Unit tests, no wiring. Done:
  `PainterCapability`/`RegisteredPainter`/`REGISTERED_PAINTERS`/`route_painter`/
  `capability_for_path`/`painter_is_available` in `src/providers/background.rs`,
  replacing the source-type `BackgroundSource`/`select_backend` selector;
  `skwd_engine::is_video_path` and `skwd_engine::engine_reachable` exposed.
  Three new tests; suite 1852 passed / 0 failed; clippy 0 warnings.
- T3 (inline) adapter: the mpvpaper plugin as a painter (local video through its
  existing manifest `local_path`). Done: `mpvpaper::paint_local_video` +
  `local_video_manifest` (pure), sharing one extracted `apply_records` with
  `apply_manifest` so every safety gate is common.
- T4 (inline) adapter: the skwd engine as a painter (the existing hand-off). Done:
  `skwd_engine::engine_reachable` gates it and `is_video_path` tells the media kind.
  `REGISTERED_PAINTERS` is preference-ordered and `route_available_painter` picks the
  first declared-and-reachable painter, so the engine keeps a local file while it is
  up and the plugin takes it when it is not.
- T5 (inline) media preparation: HVE resolves a theme's remote video to a local file
  (download + sha256) once, so no painter owns that step. Done in `eb2ddc0`:
  `background::prepare_remote_video` → `mpvpaper::fetch_remote_video` (https only,
  sha256 required, 1 GiB cap, confined video directory, url tail as file name), and the
  apply hands the local path to the router before writing the plugin's manifest, so the
  engine can paint a downloaded video too. Every refusal keeps today's manifest route.
- T6 (inline) wire the router into the theme apply so the theme's background ALWAYS
  wins: a video painter receives the theme's video, and a theme without video stops the
  running videos and paints its own static background. Done: `mpvpaper::clear_all()`
  stays on that path, because with no animated background the theme's poster is what
  must be visible.
- T7 (dropped): no event re-attach is needed. The theme's static background stays
  painted underneath whatever the keeper turns on later, so a video covers it and
  turning the video off reveals it — and a manual change is never fought (S1b).
- T8 (delegated) independent cross-model verification of the diff (writer and
  verifier are different models, per AGENTS.md). DONE (mimo-v2.6-flash, scratch
  `/tmp/opencode/wallpaper-router-verify-QARpQn`): S1, S1b, S2, S3, S4, S5, S6,
  S7, S9 PASS; S8 PARTIAL (the plugin arm has no end-to-end test because a successful
  paint runs a system-wide `pkill -9 -x mpvpaper`). ONE BLOCKER: every apply of a url
  theme re-downloaded the whole video and downloaded even with no painter available,
  where the baseline reused the file. Fixed in `6358c44` together with three
  advisories (a failing painter no longer swallows the turn; the media-kind predicate
  moved to `theme_media`; the video-backend notice dropped the disproved premise).
- T9 (inline) painter adapter for gslapper, the GStreamer renderer installed on the
  keeper's machine. Done: `src/providers/gslapper.rs` registered as a `LocalVideo`
  painter; the adapter owns the argv (`--fork --auto-pause --ipc-socket`), one socket
  per output, and answers not-available while Noctalia's own gslapper plugin is
  enabled, so two supervisors never fight.

## Log

L1 (keeper, verbatim, 25-oct-2026): "Vale y como lo hacemos para que hve no ponga en
stop a mpvpaper??? Porque seguro que mucha gente utiliza ese para cargar el fondo
con video ya que es el oficial... tenemos que poder hacer que combiban los dos, pero
eso no quiere decir que puedan estar activos los dos. Solo que si desconectas uno y
conectas el otro todo funcione correctamente."

L2 (keeper, verbatim): "Vale pero como gestionamos por ejemplo... tenemos un fondo
guardad con skwd-wall... pero luego desactivamos skwd-wall y ese tema tiene un fondo
con video... si activo ese tema mpvpaper haria que el fondo de video funcionara o se
romperia el tema?"

L3 (keeper, verbatim): "podria ser que hve mande el wallpaper de video y que lo recoja
quien este activado en ese momento independientemente de quien este al mando de
pintarlo en pantalla. Y si no hay que sea hve quien mande poner un fondo estatico del
mismo video... y que lo ponga quien este al mando de ponerlo en ese momento. HVE lanza
el fondo que tiene guardado en el tema... si no hay quien ponga un fondo animado tien
que tener en su recamara el fondo sin animar de ese fondo animado. Pero volvemos a un
problema que tenemos que resolver... HVE no se casa con nadie... tiene que ser
agnostico y asi es como tiene que ser su arquitectura. Si tenemos que hacer un
adaptador para el plugin de mpvpaper... se crea para que la logica de mpvpaper
funcione con nuestro motor agnostico."

L4 (keeper, verbatim): "vale ese es el dato que me faltaba que despues del arreglo
todos tendran url por que no se ocupan ellos de descargar si no que descarga HVE y los
demas pintan lo que les manda HVE... perfecto."

L5 (evidence, measured 25-oct-2026): with the theme JoKer applied, `hyprctl -j layers`
shows only `skwd-paper` at level 0 on DP-3 and HDMI-A-1; the mpvpaper process from
Noctalia's plugin was alive with its IPC socket but had no mapped layer. `hve.log`
records `Stopped running video wallpapers` on every apply of a theme with no video
(`background::apply_animated` → `RoutedLeg::Cleared` → `mpvpaper::clear_all()`).
`MpvpaperVideo` already carries `local_path` (`src/providers/bg_info.rs:28-39`), so the
plugin can play a local file with the format we already write; the theme packages with
video already ship `media/poster.jpg` (verified on Fantasy and Night), so the static
fallback exists in the data.

L6 (evidence): `skwd-walld`/`skwd-wall-still` accept a PATH only (`apply <path>`,
`skwd-wall-still <OUTPUT> <FILE>`); they never accept a URL. `mpvpaper` plays any local
file (the live `assignments.json` shows it launched with a local `.mp4`). That is why
URL handling belonged to the plugin adapter and why S5 moves it to HVE.

L7 (keeper, verbatim): "no se si te ha quedado claro" → answered by S9 above: the
theme apply owns the palette and the borders, the background painter has no say, and a
manual change afterwards is never fought.

L8 (progress): T2 delivered — the routing rule now reads DECLARED CAPABILITIES
(`PainterCapability`) instead of the source type, with `REGISTERED_PAINTERS` as the one
declaration place; behaviour is byte-identical to before (engine owns local video and
still, the plugin owns the saved manifest), so the full suite stayed green and no
existing test needed changing. Phase 2 (adapters) is next and needs the keeper's go.

L9 (progress): T3 + T4 delivered in commit `d007f6f` (phase 2). The plugin declares
`LocalVideo` and paints it through `paint_local_video` → `local_video_manifest` →
`apply_records` (the same body `apply_manifest` uses, so the confined video directory
and the disabled-plugin refusal are shared, not duplicated). `route_available_painter`
stops a request at the first painter that declares the capability AND can be reached,
in declaration order: the engine stays first for a local file, the plugin takes it
only when the engine is gone. RED was observed before GREEN: removing the plugin's
`LocalVideo` declaration made `a_local_video_falls_through_to_the_plugin_when_the_engine_is_absent`
fail with `None`, and restoring it made it pass. Suite 1857 passed / 0 failed, clippy 0
warnings.

L11 (keeper, verbatim, the correction that closed the design): "claro lo que tiene
que ser es que si después de aplicar el tema yo quiero cambiar el fondo por que me sale
de la nariz o de los huevos... que no haya nada que me lo impida... ahora bien... si yo
luego después de eso aplico un tema sin fondo animado siempre gana hve hasta que yo
decida manualmente que no es así."

L13 (progress): T5 delivered in `eb2ddc0`. A theme whose video is a url is now resolved
by HVE before any painter exists in the flow: `prepare_remote_video` refuses a missing
sha256, a non-https url and an unsafe file name BEFORE any network call or write, and the
downloaded file is handed to `hand_off_path` like any other video, so the engine (which
never speaks urls) can paint it too. The recorded url still lands in the plugin's
manifest, so a later boot and another machine keep working, and a refusal falls back to
today's route. Suite 1859 passed / 0 failed, clippy 0 warnings.

L14 (verification, cross-model): T8 returned 10 x PASS and S8 PARTIAL, plus one
BLOCKER: `fetch_remote_video` downloaded unconditionally, so every apply of a url theme
re-downloaded up to 1 GiB and did it even when no painter could take the video — a
regression against the baseline, which reused the file already on disk. Fixed in
`6358c44`: a matching sha256 short-circuits the download, a mismatching one is replaced,
and nothing is fetched when no painter declares and reaches the video capability. Same
commit answers three advisories: a reachable-but-failing painter (a stale engine socket)
now passes the turn to the next painter instead of swallowing it; `is_video_path` moved
from a painter's module to `theme_media`; and the url notice no longer claims "the plugin
plays no bare path". Suite 1860 passed / 0 failed, clippy 0. Two advisories stay open
and are accepted as known: the plugin arm has no end-to-end test (the system-wide
`pkill` makes it unsafe to run in a suite), and a url theme's manifest is removed after a
successful hand-off, so the doc's earlier claim about it was wrong.

L15 (progress): T9 delivered in `03dd0bc`. `gslapper` (installed on this machine,
1.5.2) is a painter now: it declares `LocalVideo`, the adapter builds
`gslapper --fork --auto-pause --ipc-socket <XDG_RUNTIME_DIR/hve-gslapper/<output>.sock>
<output> <path>`, one instance per connected output, and clears its own instances first.
While Noctalia's `nomadcxx/gslapper` plugin is enabled it answers not-available, because
that plugin is the supervisor then. Suite 1863 passed / 0 failed, clippy 0. Adapters
still NOT written, with the evidence why: spiros132's `video-wallpaper` is a v4 QML
plugin the v5 shell does not list or load (`noctalia msg plugins list` never shows it),
and `0lucasmatheus/awwwall` needs none — it mirrors the wallpaper Noctalia already has.
