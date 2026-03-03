using System;
using System.Runtime.InteropServices;
public static class KeyScanTest {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  public static extern short VkKeyScanW(char ch);
  public static int Scan(char ch) { return VkKeyScanW(ch); }
}
