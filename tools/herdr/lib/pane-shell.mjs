// Turn an argv array into one command line for the shell running in a herdr pane
// (`herdr pane run <pane> <command>` types a single string and presses Enter).

const POSIX_SAFE = /^[A-Za-z0-9_@%+=:,./-]+$/;
const PWSH_SAFE = /^[A-Za-z0-9_%+=:,./\\-]+$/;

export function defaultPaneShell() {
  return process.platform === 'win32' ? 'powershell' : 'posix';
}

export function quoteArg(arg, shell = 'posix') {
  const s = String(arg);
  if (shell === 'powershell') return PWSH_SAFE.test(s) ? s : `'${s.replace(/'/g, "''")}'`;
  if (shell === 'cmd') return /^[A-Za-z0-9_%+=:,./\\-]+$/.test(s) ? s : `"${s.replace(/"/g, '""')}"`;
  if (shell === 'posix') return s !== '' && POSIX_SAFE.test(s) ? s : `'${s.replace(/'/g, `'\\''`)}'`;
  throw new Error(`unknown pane shell "${shell}" (posix, powershell, cmd)`);
}

export function quoteCommand(argv, shell = 'posix') {
  if (!Array.isArray(argv) || argv.length === 0) throw new Error('quoteCommand needs a non-empty argv');
  const parts = argv.map((a) => quoteArg(a, shell));
  // PowerShell treats a quoted first token as a string, not a command: call it with `&`.
  if (shell === 'powershell' && parts[0].startsWith("'")) parts.unshift('&');
  return parts.join(' ');
}

// Escape a literal for herdr's `--regex` (Rust regex syntax accepts these escapes).
export function regexLiteral(s) {
  return String(s).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
