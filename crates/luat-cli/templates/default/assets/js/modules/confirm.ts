/**
 * =============================================================================
 * CUSTOM CONFIRM DIALOG MODULE (assets/js/modules/confirm.ts)
 * =============================================================================
 * Intercepts htmx:confirm events and displays a custom styled dialog
 * instead of the browser's native confirm dialog.
 *
 * USAGE:
 *   Any element with hx-confirm="message" will trigger this custom dialog.
 *   The dialog is styled with Tailwind and matches the app's glassmorphism theme.
 *
 * FEATURES:
 *   - iOS-style animations (backdrop first, then dialog pop-in)
 *   - Styled modal with backdrop blur
 *   - Dark mode support
 *   - Click outside to cancel
 *   - Accessible via native <dialog> element
 *
 * ANIMATIONS (defined in assets/css/app.css):
 *   - Opening: backdrop fades in (200ms), dialog pops in with bounce (300ms, 100ms delay)
 *   - Closing: dialog scales down (200ms), backdrop fades out (200ms, 100ms delay)
 * =============================================================================
 */

// Animation duration for closing (dialog out + backdrop out with delay)
const CLOSE_ANIMATION_DURATION = 300;

export function initConfirmDialog(): void {
  document.addEventListener("htmx:confirm", function(e: CustomEvent) {
    // Only intercept if there's a confirm question
    if (!e.detail.question) return;

    e.preventDefault();

    const dialog = document.getElementById('confirm-dialog') as HTMLDialogElement;
    const message = document.getElementById('confirm-message');
    const okBtn = document.getElementById('confirm-ok');
    const cancelBtn = document.getElementById('confirm-cancel');

    if (!dialog || !message || !okBtn || !cancelBtn) return;

    // Reset any previous closing state
    dialog.classList.remove('closing');

    message.textContent = e.detail.question;
    dialog.showModal();

    // Animated close function
    const closeWithAnimation = (callback?: () => void) => {
      dialog.classList.add('closing');

      setTimeout(() => {
        dialog.classList.remove('closing');
        dialog.close();
        cleanup();
        if (callback) callback();
      }, CLOSE_ANIMATION_DURATION);
    };

    // Handle confirm
    const handleConfirm = () => {
      closeWithAnimation(() => {
        e.detail.issueRequest(true);
      });
    };

    // Handle cancel
    const handleCancel = () => {
      closeWithAnimation();
    };

    // Handle backdrop click (clicking outside dialog)
    const handleBackdrop = (event: MouseEvent) => {
      // Check if click is on the dialog backdrop (not the content)
      const rect = dialog.getBoundingClientRect();
      const isInDialog = (
        event.clientX >= rect.left &&
        event.clientX <= rect.right &&
        event.clientY >= rect.top &&
        event.clientY <= rect.bottom
      );

      if (!isInDialog) {
        handleCancel();
      }
    };

    // Handle Escape key - prevent default and use our animated close
    const handleKeydown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        handleCancel();
      }
    };

    // Cleanup listeners
    const cleanup = () => {
      okBtn.removeEventListener('click', handleConfirm);
      cancelBtn.removeEventListener('click', handleCancel);
      dialog.removeEventListener('click', handleBackdrop);
      dialog.removeEventListener('keydown', handleKeydown);
    };

    okBtn.addEventListener('click', handleConfirm);
    cancelBtn.addEventListener('click', handleCancel);
    dialog.addEventListener('click', handleBackdrop);
    dialog.addEventListener('keydown', handleKeydown);
  } as EventListener);
}
