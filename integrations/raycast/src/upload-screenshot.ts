import { Clipboard, showHUD, showToast, Toast } from "@raycast/api";
import { buildArgs, formatLink, getPrefs, runImg } from "./utils";

export default async function uploadScreenshot() {
  await showToast({ style: Toast.Style.Animated, title: "Taking screenshot…" });
  try {
    const url = await runImg(buildArgs([], ["screenshot"]));
    const link = formatLink(url, getPrefs().outputFormat);
    await Clipboard.copy(link);
    await showHUD(`✓ Copied: ${url}`);
  } catch (e) {
    await showToast({ style: Toast.Style.Failure, title: "Upload failed", message: String(e) });
  }
}
