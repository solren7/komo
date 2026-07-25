// Ambient declaration for CSS imports (host stylesheet + the app's own
// package-subpath CSS like `streamdown/styles.css`).
// Global across the program, so it also covers @komo/app source pulled in here.
declare module "*.css";
