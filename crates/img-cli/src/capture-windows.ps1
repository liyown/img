$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Windows.Forms;
public sealed class ImgCapture : Form {
  [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window, out WinRect bounds);
  [StructLayout(LayoutKind.Sequential)] struct WinRect {public int Left,Top,Right,Bottom;}
  Bitmap screen; Point start; Rectangle selection; bool dragging; bool accepted;
  ImgCapture(Bitmap image, Rectangle bounds) {
    screen=image; FormBorderStyle=FormBorderStyle.None; StartPosition=FormStartPosition.Manual;
    Bounds=bounds; TopMost=true; DoubleBuffered=true; KeyPreview=true; Cursor=Cursors.Cross;
    MouseDown += (s,e) => {if(e.Button==MouseButtons.Left){start=e.Location;dragging=true;}};
    MouseMove += (s,e) => {if(dragging){selection=Rectangle.FromLTRB(Math.Min(start.X,e.X),Math.Min(start.Y,e.Y),Math.Max(start.X,e.X),Math.Max(start.Y,e.Y));Invalidate();}};
    MouseUp += (s,e) => {if(dragging){dragging=false;accepted=selection.Width>1 && selection.Height>1;Close();}};
    KeyDown += (s,e) => {if(e.KeyCode==Keys.Escape){accepted=false;Close();}};
  }
  protected override void OnPaint(PaintEventArgs e) {
    e.Graphics.DrawImageUnscaled(screen,0,0);
    using(var shade=new SolidBrush(Color.FromArgb(90,0,0,0))) e.Graphics.FillRectangle(shade,ClientRectangle);
    if(selection.Width>0 && selection.Height>0) {
      e.Graphics.DrawImage(screen,selection,selection,GraphicsUnit.Pixel);
      using(var pen=new Pen(Color.Orange,2)) e.Graphics.DrawRectangle(pen,selection);
    }
  }
  public static void Capture(string path,string mode) {
    SetProcessDPIAware();
    Rectangle bounds=SystemInformation.VirtualScreen;
    if(mode=="window") {
      WinRect b; if(!GetWindowRect(GetForegroundWindow(),out b)) throw new Exception("Cannot capture active window");
      bounds=Rectangle.Intersect(bounds,Rectangle.FromLTRB(b.Left,b.Top,b.Right,b.Bottom));
    }
    if(bounds.Width<1 || bounds.Height<1) throw new Exception("No visible capture area");
    using(var image=new Bitmap(bounds.Width,bounds.Height)) {
      using(var g=Graphics.FromImage(image)) g.CopyFromScreen(bounds.Location,Point.Empty,bounds.Size);
      if(mode=="region") {
        using(var overlay=new ImgCapture(image,bounds)) {
          overlay.ShowDialog();
          if(!overlay.accepted) throw new OperationCanceledException("Screenshot cancelled");
          using(var crop=image.Clone(overlay.selection,PixelFormat.Format32bppArgb)) crop.Save(path,ImageFormat.Png);
        }
      } else image.Save(path,ImageFormat.Png);
    }
  }
}
'@
if ($env:IMG_CAPTURE_VALIDATE -ne '1') {
  [ImgCapture]::Capture($env:IMG_CAPTURE_PATH,$env:IMG_CAPTURE_MODE)
}
