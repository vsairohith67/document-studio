using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;

namespace DocumentStudio.G04DC.Proof {
    public sealed class MsiConditionResult {
        public string condition { get; set; }
        public int result { get; set; }
    }

    public static class MsiConditionEvaluator {
        private const uint MSIOPENPACKAGEFLAGS_IGNOREMACHINESTATE = 0x1;

        [DllImport("msi.dll", CharSet = CharSet.Unicode)]
        private static extern uint MsiOpenPackageEx(string packagePath, uint options, out uint product);
        [DllImport("msi.dll", CharSet = CharSet.Unicode)]
        private static extern uint MsiSetProperty(uint install, string name, string value);
        [DllImport("msi.dll", CharSet = CharSet.Unicode)]
        private static extern int MsiEvaluateCondition(uint install, string condition);
        [DllImport("msi.dll")]
        private static extern uint MsiCloseHandle(uint handle);

        public static MsiConditionResult[] Evaluate(string packagePath, string[] propertyNames, string[] propertyValues, string[] conditions) {
            if (propertyNames == null || propertyValues == null || propertyNames.Length != propertyValues.Length) {
                throw new ArgumentException("MSI property names and values must have equal length.");
            }
            uint package;
            uint status = MsiOpenPackageEx(packagePath, MSIOPENPACKAGEFLAGS_IGNOREMACHINESTATE, out package);
            if (status != 0) throw new Win32Exception((int)status, "MsiOpenPackageEx failed");
            try {
                for (int index = 0; index < propertyNames.Length; index++) {
                    status = MsiSetProperty(package, propertyNames[index], propertyValues[index]);
                    if (status != 0) throw new Win32Exception((int)status, "MsiSetProperty failed for " + propertyNames[index]);
                }
                List<MsiConditionResult> results = new List<MsiConditionResult>();
                foreach (string condition in conditions) {
                    results.Add(new MsiConditionResult { condition = condition, result = MsiEvaluateCondition(package, condition) });
                }
                return results.ToArray();
            }
            finally { MsiCloseHandle(package); }
        }
    }
}
