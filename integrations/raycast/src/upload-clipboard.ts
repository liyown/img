import { Clipboard, showHUD, showToast, Toast } from "@raycast/api";
import { execFile } from "child_process";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";
import { buildArgs, formatLink, getPrefs, runImg } from "./utils";

export default async function uploadClipboard() {
  await showToast({ style: Toast.Style.Animated, title: "Uploading clipboard image…" });

  const tmp = path.join(os.tmpdir(), `img-clipboard-${Date.now()}.png`);
  try {
    await saveClipboardImage(tmp);
    const url = await runImg(buildArgs([tmp]));
    const link = formatLink(url, getPrefs().outputFormat);
    await Clipboard.copy(link);
    await showHUD(`✓ Copied: ${url}`);
  } catch (e) {
    await showToast({ style: Toast.Style.Failure, title: "Upload failed", message: String(e) });
  } finally {
    fs.unlink(tmp, () => {});
  }
}

function saveClipboardImage(dest: string): Promise<void> {
  return new Promise((resolve, reject) => {
    // macOS: use osascript to read PNG from clipboard
    execFile(
      "osascript",
      ["-e", `set d to (open for access POSIX file "${dest}" with write permission)\nwrite (the clipboard as «class PNGf») to d\nclose access d`],
      (err, _out, stderr) => {
        if (err) return reject(new Error(stderr || err.message));
        if (!fs.existsSync(dest) || fs.statSync(dest).size === 0)
          return reject(new Error("No image found in clipboard"));
        resolve();
      }
    );
  });
}
