/**
 * Todo list keyboard navigation
 *
 * Only arrow key navigation is handled in JS - all other keyboard
 * interactions are done declaratively via HTMX hx-trigger attributes.
 */

export function initTodos(): void {
  // Only arrow key navigation needs JS
  document.addEventListener('keydown', handleArrowNavigation);
}

function handleArrowNavigation(event: KeyboardEvent): void {
  const target = event.target as HTMLElement;
  const todoItem = target.closest('li[id^="todo-"]') as HTMLElement;

  if (!todoItem) return;

  // Skip if we're in an input field
  if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') {
    return;
  }

  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault();
      const nextItem = todoItem.nextElementSibling as HTMLElement;
      nextItem?.focus();
      break;

    case 'ArrowUp':
      event.preventDefault();
      const prevItem = todoItem.previousElementSibling as HTMLElement;
      prevItem?.focus();
      break;
  }
}

// Focus management after HTMX swaps - needed for accessibility
export function initFocusManagement(): void {
  document.body.addEventListener('htmx:afterSwap', (event: Event) => {
    const detail = (event as CustomEvent).detail;
    const target = detail?.target as HTMLElement;

    if (target?.id === 'todo-list' || target?.id?.startsWith('todo-')) {
      // Focus the new element if it's a todo item
      const newElement = document.getElementById(target.id);
      if (newElement?.tagName === 'LI' && !newElement.hasAttribute('data-editing')) {
        newElement.focus();
      }
      // Focus the input if we're in edit mode
      const editInput = newElement?.querySelector('input[type="text"]:not([type="hidden"])') as HTMLInputElement;
      if (editInput) {
        editInput.focus();
        editInput.select();
      }
    }
  });
}
