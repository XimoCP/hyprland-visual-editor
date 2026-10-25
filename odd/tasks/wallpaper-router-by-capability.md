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

S1 (keeper, verbatim): "tenemos que poder hacer que combiban los dos, pero eso no
quiere decir que puedan estar activos los dos. Solo que si desconectas uno y
conectas el otro todo funcione correctamente." HVE never stops or withdraws a
painter it did not hand the current background to, at most one painter paints the
theme's background at a time, and switching one off and the other on works in both
directions.

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
  existing manifest `local_path`).
- T4 (inline) adapter: the skwd engine as a painter (the existing hand-off).
- T5 (inline) media preparation: HVE resolves a theme's remote video to a local file
  (download + sha256) once, so no painter owns that step.
- T6 (inline) wire the router into the theme apply: no `clear_all()`, one painter
  per background, static fallback.
- T7 (inline) re-attach on switch: a video painter appearing retires HVE's poster, a
  video painter going away re-paints it — from the layer events already listened to,
  no polling.
- T8 (delegated) independent cross-model verification of the diff (writer and
  verifier are different models, per AGENTS.md).

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
