using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class InjectTest {
  private const int STD_INPUT_HANDLE = -10;
  private const short KEY_EVENT = 0x0001;
  [StructLayout(LayoutKind.Explicit, CharSet = CharSet.Unicode)]
  public struct INPUT_RECORD {
    [FieldOffset(0)] public short EventType;
    [FieldOffset(4)] public KEY_EVENT_RECORD KeyEvent;
  }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct KEY_EVENT_RECORD {
    [MarshalAs(UnmanagedType.Bool)] public bool bKeyDown;
    public ushort wRepeatCount;
    public ushort wVirtualKeyCode;
    public ushort wVirtualScanCode;
    public char UnicodeChar;
    public uint dwControlKeyState;
  }
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern IntPtr GetStdHandle(int nStdHandle);
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern bool WriteConsoleInputW(IntPtr hConsoleInput, INPUT_RECORD[] lpBuffer, uint nLength, out uint lpNumberOfEventsWritten);

  public static string TryChar(char c, ushort vk) {
    var h = GetStdHandle(STD_INPUT_HANDLE);
    var down = new INPUT_RECORD{ EventType = KEY_EVENT, KeyEvent = new KEY_EVENT_RECORD{ bKeyDown=true, wRepeatCount=1, wVirtualKeyCode=vk, wVirtualScanCode=0, UnicodeChar=c, dwControlKeyState=0}};
    var up = down; up.KeyEvent.bKeyDown=false; up.KeyEvent.UnicodeChar='\0';
    var arr = new INPUT_RECORD[]{down,up};
    uint written;
    bool ok = WriteConsoleInputW(h, arr, (uint)arr.Length, out written);
    int err = Marshal.GetLastWin32Error();
    return $"ok={ok} written={written} err={err}";
  }
}
