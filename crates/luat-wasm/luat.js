/**
 * Luat WASM JavaScript Wrapper
 *
 * This module provides a clean JavaScript API for the Luat templating engine
 * compiled to WebAssembly via Emscripten.
 *
 * Usage in browser with script tag:
 *   <script src="luat-wasm.js"></script>
 *   <script type="module">
 *     import { initLuatWithModule } from './luat.js';
 *
 *     const luat = await initLuatWithModule(window.Module);
 *     luat.addTemplate('main.luat', '<h1>Hello, {props.name}!</h1>');
 *     const html = luat.render('main.luat', { name: 'World' });
 *   </script>
 *
 * Or for direct use with automatic module loading:
 *   import { loadLuat } from './luat.js';
 *
 *   const luat = await loadLuat('/path/to/wasm/');
 */

let Module = null;
let initialized = false;

/**
 * Initialize Luat with a pre-loaded Emscripten Module.
 * Use this when you've already loaded luat-wasm.js via script tag.
 *
 * @param {Function} createModule - The Module factory function from luat-wasm.js
 * @param {Object} options - Options including locateFile for WASM path
 * @returns {Promise<LuatAPI>} The Luat API object
 */
export async function initLuatWithModule(createModule, options = {}) {
  if (initialized) {
    return createAPI();
  }

  Module = await createModule(options);

  // Initialize the Luat engine
  const result = Module.ccall('luat_init', 'number', [], []);
  if (result !== 0) {
    throw new Error('Failed to initialize Luat engine');
  }

  initialized = true;
  return createAPI();
}

/**
 * Load Luat WASM module from a URL base path.
 * This will dynamically load the scripts.
 *
 * @param {string} basePath - The base URL path where luat-wasm.js and luat_wasm.wasm are located
 * @returns {Promise<LuatAPI>} The Luat API object
 */
export async function loadLuat(basePath) {
  if (initialized) {
    return createAPI();
  }

  // Ensure basePath ends with /
  const base = basePath.endsWith('/') ? basePath : basePath + '/';

  // Load the Emscripten module via script tag
  await new Promise((resolve, reject) => {
    const script = document.createElement('script');
    script.src = `${base}luat-wasm.js`;
    script.onload = resolve;
    script.onerror = () => reject(new Error('Failed to load luat-wasm.js'));
    document.head.appendChild(script);
  });

  // Get the factory function (it's a global after script load)
  const createModule = window.Module;
  if (!createModule) {
    throw new Error('Module not found after loading script');
  }

  return initLuatWithModule(createModule, {
    locateFile: (path) => `${base}${path}`
  });
}

/**
 * Create the Luat API object with all methods.
 */
function createAPI() {
  return {
    addTemplate,
    removeTemplate,
    clearTemplates,
    render,
    renderWithError,
    version,
    isInitialized: () => initialized,
  };
}

/**
 * Add a template to the engine's memory.
 */
function addTemplate(path, source) {
  if (!initialized) {
    throw new Error('Luat not initialized');
  }

  const result = Module.ccall(
    'luat_add_template',
    'number',
    ['string', 'string'],
    [path, source]
  );

  if (result !== 0) {
    throw new Error(`Failed to add template: ${path}`);
  }
}

/**
 * Remove a template from the engine's memory.
 */
function removeTemplate(path) {
  if (!initialized) {
    throw new Error('Luat not initialized');
  }

  Module.ccall(
    'luat_remove_template',
    'number',
    ['string'],
    [path]
  );
}

/**
 * Clear all templates from the engine's memory.
 */
function clearTemplates() {
  if (!initialized) {
    throw new Error('Luat not initialized');
  }

  Module.ccall('luat_clear_templates', 'number', [], []);
}

/**
 * Render a template with the given context.
 */
function render(entry, context = {}) {
  if (!initialized) {
    throw new Error('Luat not initialized');
  }

  const contextJson = JSON.stringify(context);
  const resultPtr = Module.ccall(
    'luat_render',
    'number',
    ['string', 'string'],
    [entry, contextJson]
  );

  if (resultPtr === 0) {
    throw new Error(`Failed to render template: ${entry}`);
  }

  const result = Module.UTF8ToString(resultPtr);
  Module._luat_free_string(resultPtr);
  return result;
}

/**
 * Render a template with detailed error information.
 */
function renderWithError(entry, context = {}) {
  if (!initialized) {
    throw new Error('Luat not initialized');
  }

  const contextJson = JSON.stringify(context);
  const resultPtr = Module.ccall(
    'luat_render_with_error',
    'number',
    ['string', 'string'],
    [entry, contextJson]
  );

  if (resultPtr === 0) {
    return {
      success: false,
      html: null,
      error: 'Internal error: null result from WASM'
    };
  }

  const resultJson = Module.UTF8ToString(resultPtr);
  Module._luat_free_string(resultPtr);
  return JSON.parse(resultJson);
}

/**
 * Get the version of the Luat library.
 */
function version() {
  if (!initialized) {
    throw new Error('Luat not initialized');
  }

  const versionPtr = Module._luat_version();
  return Module.UTF8ToString(versionPtr);
}

// Default export
export default {
  initLuatWithModule,
  loadLuat,
  addTemplate,
  removeTemplate,
  clearTemplates,
  render,
  renderWithError,
  version,
};
