# Fix doble click — Fase 1 + 2

Aprobado por usuario.

## Fase 1 — Restaurar funcionalidad

### PresetCard
- Restaurar `fs.has-focus` en title color y desc wrap (NO en height/background/border-color)
- Volver de `pointer-event` a `clicked`
- TouchArea se queda al final del componente

### NavButton
- Restaurar `nb-fs.has-focus` en background
- Volver de `pointer-event` a `clicked`
- TouchArea se queda al final

## Fase 2 — Aislar causa raíz (si doble click persiste)

Probar en orden, un cambio a la vez:
1. Sacar `animate height` (altura fija 68px)
2. Sacar easing `ease-in-out-back`
3. Sacar `clip: true`
4. Sacar `animate background`, `border-color`, `drop-shadow-blur`

Cada paso se compila, prueba, y si soluciona el problema → nos quedamos sin eso.
