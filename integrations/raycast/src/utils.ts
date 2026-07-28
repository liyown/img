import { execFile } from "child_process";
import { getPreferenceValues } from "@raycast/api";

interface Preferences {
  imgPath: string;
  outputFormat: "markdown" | "url" | "html";
  provider: string;
  optimize: boolean;
  stripExif: boolean;
}

export function getPrefs(): Preferences {
  return getPreferenceValues<Preferences>();
}

export function buildArgs(files: string[], extra: string[] = []): string[] {
  const prefs = getPrefs();
  const args = [...files, "--format", "url", ...extra];
  if (prefs.provider) args.push("--provider", prefs.provider);
  if (prefs.optimize) args.push("--optimize");
  if (prefs.stripExif) args.push("--strip-exif");
  return args;
}

export function formatLink(url: string, format: "markdown" | "url" | "html"): string {
  const filename = url.split("/").pop() ?? "image";
  switch (format) {
    case "markdown": return `![${filename}](${url})`;
    case "html":     return `<img src="${url}" alt="${filename}">`;
    default:         return url;
  }
}

export function runImg(args: string[]): Promise<string> {
  const prefs = getPrefs();
  const bin = prefs.imgPath || "img";
  return new Promise((resolve, reject) => {
    execFile(bin, args, (err, stdout, stderr) => {
      if (err) reject(new Error(stderr || err.message));
      else resolve(stdout.trim());
    });
  });
}
