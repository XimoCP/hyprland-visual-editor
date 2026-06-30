# Fix: doble click en PresetCard / NavButton

## Diagnóstico

Los componentes PresetCard y NavButton incluyen `fs.has-focus` / `nb-fs.has-focus`
en las condiciones de hover/expansión. Al clickear sin que la ventana tenga foco,
el FocusScope agarra foco pero el click se pierde → el card se queda "trabado"
en expandido.

## Cambios

### `ui/components.slint` — PresetCard (líneas 13, 16, 18)

- `height` quita `fs.has-focus`
- `background` quita `fs.has-focus`  
- `border-color` quita `fs.has-focus`

### `ui/components.slint` — NavButton (línea 155)

- `background` quita `nb-fs.has-focus`

### `src/main.rs` — ya implementado

`Timer::single_shot` + `hyprctl dispatch focuswindow title:Hyprland Visual Editor`
200ms después de `window.show()`.

## No se toca

- FocusScope permanece (navegación por teclado)
- Lógica de negocio en callbacks.rs
- Countdown auto-close

## Verificación

- `cargo build` sin warnings
- `cargo test` 66 tests pasan
