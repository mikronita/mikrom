• Sí. En mikrom-app hay varias mejoras de buen retorno, y yo las priorizaría así:

  1. Gestión real de proyectos

  - Añadir renombrado de proyecto.
  - Añadir eliminación de proyecto con confirmación.
  - Añadir gestión de miembros del proyecto si el backend lo permite.
  - Mostrar metadatos del proyecto: slug, fecha, miembros, rol actual.

  2. Mejor contexto multi-tenant

  - Persistir mejor el proyecto activo en URL o en localStorage además de cookie.
  - Añadir fallback claro si el proyecto guardado ya no existe.
  - Mostrar el proyecto activo en más pantallas, no solo en el switcher y el overlay.

  3. Mejor UX en navegación

  - Añadir búsqueda global dentro de apps/proyectos.
  - Añadir filtros por estado en apps, VMs y volúmenes.
  - Mantener breadcrumbs más consistentes en todas las páginas.
  - Hacer que las transiciones de cambio de proyecto sean más suaves en mobile también.

  4. Onboarding y creación de apps

  - Mejorar la modal de crear app para mostrar el proyecto activo y el scope real.
  - Añadir plantillas de despliegue o ejemplos de repo.
  - Añadir validación más proactiva de GitHub/Git URL.
  - Mostrar errores de creación con mensajes más accionables.

  5. Observabilidad y estado

  - Unificar skeletons, empty states y error states en todo el app shell.
  - Añadir auto-refresh visual cuando cambie el proyecto.
  - Mostrar “last updated” en listas importantes.
  - Añadir notificaciones más específicas cuando cambian apps/VMs por SSE.

  6. Storage y networking

  - Corregir y pulir la vista de storage/[volumeId] que ya tiene más deuda visual.
  - Añadir acciones inline para snapshots y attachments.
  - Hacer el networking panel más orientado a app/proyecto.
  - Mejorar la gestión de reglas de seguridad con edición y orden.

  7. Calidad técnica

  - Eliminar componentes duplicados o wrappers innecesarios.
  - Tipar mejor los componentes base para evitar repetir fixes como los de select/badge.
  - Añadir tests de navegación multi-tenant, sobre todo al cambiar de proyecto.
  - Reducir lógica de refresco dispersa y centralizarla más.

  Si tuviera que elegir el siguiente paso más útil, haría uno de estos dos:

  1. Gestion de miembros y edición de proyectos.
  2. Mejor onboarding de creación y despliegue de apps dentro del proyecto activo.

  Si quieres, te propongo un roadmap de 3 iteraciones concretas para mikrom-app.
