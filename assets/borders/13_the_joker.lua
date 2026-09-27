-- @Title: the_joker
-- @Icon: mood-sad
-- @Color: #39ff14
-- @Tag: THEME
-- @Desc: Joker Aesthetic: Acid green and deep purple with electric glow.
-- =====================================
---@diagnostic disable: undefined-global

local joker_green  = "rgba(39ff14ff)"
local joker_purple = "rgba(9d00ffff)"
local joker_dark   = "rgba(1a0026ff)"

-- Keep the original purple shadow.
local shadow_glow  = "rgba(9d00ff88)"
-- Same purple, alpha "00" so the shadow is invisible.
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
            range = 20,                  -- Original range.
            render_power = 4,            -- Original power.
            color = shadow_glow,
            color_inactive = shadow_off, -- Hyprland fades it out until it is invisible.
            offset = { 0, 0 }
        }
    },
    windowrulev2 = {
        "noshadow, focus:0",
        "dim_around, floating:1" -- Dim around floating windows.
    }
})

hl.curve("nv_joker_flow", { type = "bezier", points = { { 0.4, 0 }, { 0.2, 1 } } })
hl.curve("joker_bounce", { type = "bezier", points = { { 0.175, 0.885 }, { 0.32, 1.275 } } })

hl.animation({ leaf = "borderangle", enabled = true, speed = 30, bezier = "nv_joker_flow", style = "loop" })
-- The "border" animation controls how fast the shadow fades out.
hl.animation({ leaf = "border", enabled = true, speed = 30, bezier = "default" })
-- Shadow fade when switching windows.
hl.animation({ leaf = "fadeShadow", enabled = true, speed = 10, bezier = "default" })
