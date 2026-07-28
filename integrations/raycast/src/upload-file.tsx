import { Action, ActionPanel, Clipboard, Form, showHUD, showToast, Toast, useNavigation } from "@raycast/api";
import { useState } from "react";
import { buildArgs, formatLink, getPrefs, runImg } from "./utils";

export default function UploadFile() {
  const [files, setFiles] = useState<string[]>([]);
  const [uploading, setUploading] = useState(false);
  const { pop } = useNavigation();

  async function handleSubmit() {
    if (files.length === 0) return;
    setUploading(true);
    await showToast({ style: Toast.Style.Animated, title: `Uploading ${files.length} file(s)…` });
    try {
      const output = await runImg(buildArgs(files));
      const urls = output.split("\n").filter(Boolean);
      const prefs = getPrefs();
      const links = urls.map(u => formatLink(u, prefs.outputFormat)).join("\n");
      await Clipboard.copy(links);
      await showHUD(`✓ Copied ${urls.length} link(s)`);
      pop();
    } catch (e) {
      await showToast({ style: Toast.Style.Failure, title: "Upload failed", message: String(e) });
    } finally {
      setUploading(false);
    }
  }

  return (
    <Form
      isLoading={uploading}
      actions={
        <ActionPanel>
          <Action.SubmitForm title="Upload" onSubmit={handleSubmit} />
        </ActionPanel>
      }
    >
      <Form.FilePicker
        id="files"
        title="Image files"
        allowMultipleSelection
        canChooseDirectories={false}
        value={files}
        onChange={setFiles}
      />
    </Form>
  );
}
