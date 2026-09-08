-- @Title: the_joker
-- @Icon: mood-sad
-- @Tag: THEME
-- @Desc: Joker Aesthetic: Acid green and deep purple with electric glow.
-- =====================================
---@diagnostic disable: undefined-global

local joker_green  = "rgba(39ff14ff)"
local joker_purple = "rgba(9d00ffff)"
local joker_dark   = "rgba(1a0026ff)"

-- Mantenemos tu sombra morada original
local shadow_glow  = "rgba(9d00ff88)"
-- El mismo color morado, pero con opacidad "00" para que sea invisible
local shadow_off   = "rgba(9d00ff00)"

hl.config({
    general = {
        col = {
            active_border = {
                colors = { joker_green, joker_dark, joker_purple },
                angle = 45
            },
            inactive_border = "rgba(1a002655)"
        },
    },
    decoration = {
        shadow = {
            enabled = true,
            range = 20,                  -- Tu rango original
            render_power = 4,            -- Tu potencia original
            color = shadow_glow,
            color_inactive = shadow_off, -- Hyprland hará un fundido hasta volverla invisible
            offset = { 0, 0 }
        }
    },
    windowrulev2 = {
        "noshadow, focus:0",
        "dim_around, floating:1" -- NUEVO: dim around floating windows
    }
})

hl.curve("nv_joker_flow", { type = "bezier", points = { { 0.4, 0 }, { 0.2, 1 } } })
hl.curve("joker_bounce", { type = "bezier", points = { { 0.175, 0.885 }, { 0.32, 1.275 } } })

hl.animation({ leaf = "borderangle", enabled = true, speed = 30, bezier = "nv_joker_flow", style = "loop" })
-- Esta animación "border" es la que controla lo rápido que se apaga la sombra
hl.animation({ leaf = "border", enabled = true, speed = 30, bezier = "default" })
-- NUEVO: Fade de la sombra al cambiar de ventana
hl.animation({ leaf = "fadeShadow", enabled = true, speed = 10, bezier = "default" })
