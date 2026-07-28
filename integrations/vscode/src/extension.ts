import * as vscode from 'vscode';
import * as cp from 'child_process';
import * as os from 'os';
import * as path from 'path';
import * as fs from 'fs';

export function activate(context: vscode.ExtensionContext) {
    context.subscriptions.push(
        vscode.commands.registerCommand('img.pasteImage', pasteImage),
        vscode.commands.registerCommand('img.uploadFile', uploadFile),
        vscode.commands.registerCommand('img.uploadFileFromExplorer', uploadFileFromExplorer),
    );
}

export function deactivate() {}

// ── Paste clipboard image and upload ────────────────────────────────────────

async function pasteImage() {
    const editor = vscode.window.activeTextEditor;
    if (!editor) return;

    // Save clipboard image to a temp file, then upload.
    const tmp = path.join(os.tmpdir(), `img-paste-${Date.now()}.png`);
    try {
        await saveClipboardImage(tmp);
    } catch (e) {
        vscode.window.showErrorMessage(`img: ${e}`);
        return;
    }

    try {
        const result = await runImg([tmp], { format: 'url' });
        const link = formatOutput(result.trim(), getConfig('outputFormat', 'markdown'));
        await editor.edit(b => b.insert(editor.selection.active, link));
    } catch (e) {
        vscode.window.showErrorMessage(`img upload failed: ${e}`);
    } finally {
        fs.unlink(tmp, () => {});
    }
}

// ── Upload a file via command palette ────────────────────────────────────────

async function uploadFile() {
    const uris = await vscode.window.showOpenDialog({
        canSelectMany: true,
        filters: { Images: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'avif'] },
    });
    if (!uris || uris.length === 0) return;

    const paths = uris.map(u => u.fsPath);
    await uploadAndInsert(paths);
}

// ── Right-click upload from Explorer ────────────────────────────────────────

async function uploadFileFromExplorer(uri: vscode.Uri) {
    await uploadAndInsert([uri.fsPath]);
}

// ── Core upload helper ───────────────────────────────────────────────────────

async function uploadAndInsert(files: string[]) {
    await vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: 'img: uploading…', cancellable: false },
        async () => {
            try {
                const result = await runImg(files, { format: 'url' });
                const lines = result.trim().split('\n').filter(Boolean);
                const fmt = getConfig<string>('outputFormat', 'markdown');
                const links = lines.map(url => formatOutput(url, fmt)).join('\n');

                const editor = vscode.window.activeTextEditor;
                if (editor) {
                    await editor.edit(b => b.insert(editor.selection.active, links));
                } else {
                    await vscode.env.clipboard.writeText(links);
                    vscode.window.showInformationMessage('img: copied to clipboard — ' + lines[0]);
                }
            } catch (e) {
                vscode.window.showErrorMessage(`img upload failed: ${e}`);
            }
        }
    );
}

// ── Run the img CLI ──────────────────────────────────────────────────────────

function runImg(files: string[], opts: { format?: string } = {}): Promise<string> {
    return new Promise((resolve, reject) => {
        const bin = getConfig<string>('executablePath', 'img');
        const args: string[] = [...files, '--format', opts.format ?? 'url'];

        const provider = getConfig<string>('provider', '');
        if (provider) args.push('--provider', provider);
        if (getConfig<boolean>('optimize', false)) args.push('--optimize');
        if (getConfig<boolean>('stripExif', false)) args.push('--strip-exif');

        cp.execFile(bin, args, (err, stdout, stderr) => {
            if (err) {
                reject(stderr || err.message);
            } else {
                resolve(stdout);
            }
        });
    });
}

// ── Save clipboard image to disk (macOS / Linux / Windows) ──────────────────

function saveClipboardImage(destPath: string): Promise<void> {
    return new Promise((resolve, reject) => {
        let cmd: string;
        let args: string[];

        switch (process.platform) {
            case 'darwin':
                // Use osascript to write clipboard PNG to file.
                cmd = 'osascript';
                args = [
                    '-e',
                    `set png_data to the clipboard as «class PNGf»
                     set fp to open for access POSIX file "${destPath}" with write permission
                     set eof fp to 0
                     write png_data to fp
                     close access fp`,
                ];
                break;
            case 'linux':
                cmd = 'xclip';
                args = ['-selection', 'clipboard', '-t', 'image/png', '-o'];
                break;
            case 'win32':
                cmd = 'powershell';
                args = [
                    '-NoProfile', '-NonInteractive', '-Command',
                    `Add-Type -AssemblyName System.Windows.Forms;` +
                    `$img=[System.Windows.Forms.Clipboard]::GetImage();` +
                    `if(!$img){exit 1}` +
                    `$img.Save('${destPath}',[System.Drawing.Imaging.ImageFormat]::Png)`,
                ];
                break;
            default:
                return reject(new Error(`Clipboard image paste is not supported on ${process.platform}`));
        }

        cp.execFile(cmd, args, (err, _stdout, stderr) => {
            if (err) return reject(stderr || err.message);
            if (!fs.existsSync(destPath) || fs.statSync(destPath).size === 0) {
                return reject(new Error('No image found in clipboard'));
            }
            resolve();
        });
    });
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function getConfig<T>(key: string, defaultValue: T): T {
    return vscode.workspace.getConfiguration('img').get<T>(key, defaultValue);
}

function formatOutput(url: string, format: string): string {
    const filename = url.split('/').pop() ?? 'image';
    switch (format) {
        case 'markdown': return `![${filename}](${url})`;
        case 'html':     return `<img src="${url}" alt="${filename}">`;
        default:         return url;
    }
}
