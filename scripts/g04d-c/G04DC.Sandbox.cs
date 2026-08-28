using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;

namespace DocumentStudio.G04DC.Proof {
    public sealed class ProcessRecord {
        public int pid { get; set; }
        public string path { get; set; }
        public string name { get; set; }
    }

    public sealed class ModuleRecord {
        public int pid { get; set; }
        public string path { get; set; }
    }

    public sealed class SandboxEvidence {
        public string profileName { get; set; }
        public string appContainerSid { get; set; }
        public bool appContainer { get; set; }
        public string tokenAppContainerSid { get; set; }
        public string[] capabilities { get; set; }
        public bool assignedBeforeResume { get; set; }
        public bool breakawayAllowed { get; set; }
        public int activeProcessLimit { get; set; }
        public int totalAssignedProcesses { get; set; }
        public int peakAssignedProcessCount { get; set; }
        public long aggregateMemoryLimitBytes { get; set; }
        public long peakJobMemoryBytes { get; set; }
        public int rootPid { get; set; }
        public int exitCode { get; set; }
        public bool timedOut { get; set; }
        public bool profileDeleted { get; set; }
        public ProcessRecord[] processes { get; set; }
        public ModuleRecord[] loadedModules { get; set; }
        public bool moduleInventoryComplete { get; set; }
        public string[] networkConnections { get; set; }
    }

    public sealed class SandboxRunException : Exception {
        public SandboxEvidence Evidence { get; private set; }
        public SandboxRunException(string message, Exception inner, SandboxEvidence evidence) : base(message, inner) {
            Evidence = evidence;
        }
    }

    public static class OfficeSandbox {
        private const uint CREATE_SUSPENDED = 0x00000004;
        private const uint CREATE_NO_WINDOW = 0x08000000;
        private const uint EXTENDED_STARTUPINFO_PRESENT = 0x00080000;
        private const uint PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES = 0x00020005;
        private const uint JOB_OBJECT_LIMIT_ACTIVE_PROCESS = 0x00000008;
        private const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
        private const uint JOB_OBJECT_LIMIT_JOB_MEMORY = 0x00000200;
        private const uint TOKEN_QUERY = 0x0008;
        private const int TokenIsAppContainer = 29;
        private const int TokenCapabilities = 30;
        private const int TokenAppContainerSid = 31;
        private const uint WAIT_OBJECT_0 = 0;
        private const uint WAIT_TIMEOUT = 258;

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct STARTUPINFO {
            public int cb;
            public string lpReserved;
            public string lpDesktop;
            public string lpTitle;
            public int dwX;
            public int dwY;
            public int dwXSize;
            public int dwYSize;
            public int dwXCountChars;
            public int dwYCountChars;
            public int dwFillAttribute;
            public int dwFlags;
            public short wShowWindow;
            public short cbReserved2;
            public IntPtr lpReserved2;
            public IntPtr hStdInput;
            public IntPtr hStdOutput;
            public IntPtr hStdError;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct STARTUPINFOEX {
            public STARTUPINFO StartupInfo;
            public IntPtr lpAttributeList;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct PROCESS_INFORMATION {
            public IntPtr hProcess;
            public IntPtr hThread;
            public int dwProcessId;
            public int dwThreadId;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct SECURITY_CAPABILITIES {
            public IntPtr AppContainerSid;
            public IntPtr Capabilities;
            public int CapabilityCount;
            public int Reserved;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
            public long PerProcessUserTimeLimit;
            public long PerJobUserTimeLimit;
            public uint LimitFlags;
            public UIntPtr MinimumWorkingSetSize;
            public UIntPtr MaximumWorkingSetSize;
            public uint ActiveProcessLimit;
            public UIntPtr Affinity;
            public uint PriorityClass;
            public uint SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct IO_COUNTERS {
            public ulong ReadOperationCount;
            public ulong WriteOperationCount;
            public ulong OtherOperationCount;
            public ulong ReadTransferCount;
            public ulong WriteTransferCount;
            public ulong OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct JOBOBJECT_BASIC_ACCOUNTING_INFORMATION {
            public long TotalUserTime;
            public long TotalKernelTime;
            public long ThisPeriodTotalUserTime;
            public long ThisPeriodTotalKernelTime;
            public uint TotalPageFaultCount;
            public uint TotalProcesses;
            public uint ActiveProcesses;
            public uint TotalTerminatedProcesses;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
            public IO_COUNTERS IoInfo;
            public UIntPtr ProcessMemoryLimit;
            public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed;
            public UIntPtr PeakJobMemoryUsed;
        }

        [DllImport("userenv.dll", CharSet = CharSet.Unicode)]
        private static extern int CreateAppContainerProfile(string name, string displayName, string description, IntPtr capabilities, uint capabilityCount, out IntPtr sid);
        [DllImport("userenv.dll", CharSet = CharSet.Unicode)]
        private static extern int DeleteAppContainerProfile(string name);
        [DllImport("advapi32.dll", SetLastError = true)]
        private static extern bool ConvertSidToStringSid(IntPtr sid, out IntPtr text);
        [DllImport("advapi32.dll", SetLastError = true)]
        private static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
        [DllImport("advapi32.dll", SetLastError = true)]
        private static extern bool GetTokenInformation(IntPtr token, int infoClass, IntPtr information, int informationLength, out int returnLength);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool InitializeProcThreadAttributeList(IntPtr list, int count, int flags, ref IntPtr size);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool UpdateProcThreadAttribute(IntPtr list, uint flags, IntPtr attribute, IntPtr value, IntPtr size, IntPtr previousValue, IntPtr returnSize);
        [DllImport("kernel32.dll")]
        private static extern void DeleteProcThreadAttributeList(IntPtr list);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool CreateProcess(string applicationName, StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes, bool inheritHandles, uint flags, IntPtr environment, string currentDirectory, ref STARTUPINFOEX startupInfo, out PROCESS_INFORMATION processInformation);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr CreateJobObject(IntPtr attributes, string name);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool SetInformationJobObject(IntPtr job, int infoClass, IntPtr information, uint informationLength);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool QueryInformationJobObject(IntPtr job, int infoClass, IntPtr information, uint informationLength, out uint returnLength);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint ResumeThread(IntPtr thread);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool TerminateJobObject(IntPtr job, uint exitCode);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool CloseHandle(IntPtr handle);
        [DllImport("kernel32.dll")]
        private static extern IntPtr LocalFree(IntPtr memory);
        [DllImport("advapi32.dll", CallingConvention = CallingConvention.StdCall)]
        private static extern IntPtr FreeSid(IntPtr sid);

        private static void Win32(bool result, string operation) {
            if (!result) throw new Win32Exception(Marshal.GetLastWin32Error(), operation);
        }

        private static string SidText(IntPtr sid) {
            IntPtr text;
            Win32(ConvertSidToStringSid(sid, out text), "ConvertSidToStringSid");
            try { return Marshal.PtrToStringUni(text); }
            finally { LocalFree(text); }
        }

        private static Tuple<bool, string, string[]> TokenEvidence(IntPtr process) {
            IntPtr token;
            Win32(OpenProcessToken(process, TOKEN_QUERY, out token), "OpenProcessToken");
            try {
                int needed;
                IntPtr booleanBuffer = Marshal.AllocHGlobal(4);
                try {
                    Win32(GetTokenInformation(token, TokenIsAppContainer, booleanBuffer, 4, out needed), "GetTokenInformation(TokenIsAppContainer)");
                    bool isAppContainer = Marshal.ReadInt32(booleanBuffer) != 0;
                    GetTokenInformation(token, TokenAppContainerSid, IntPtr.Zero, 0, out needed);
                    IntPtr sidBuffer = Marshal.AllocHGlobal(needed);
                    string tokenSid;
                    try {
                        Win32(GetTokenInformation(token, TokenAppContainerSid, sidBuffer, needed, out needed), "GetTokenInformation(TokenAppContainerSid)");
                        tokenSid = SidText(Marshal.ReadIntPtr(sidBuffer));
                    } finally { Marshal.FreeHGlobal(sidBuffer); }
                    GetTokenInformation(token, TokenCapabilities, IntPtr.Zero, 0, out needed);
                    IntPtr groups = Marshal.AllocHGlobal(Math.Max(needed, 4));
                    try {
                        Win32(GetTokenInformation(token, TokenCapabilities, groups, Math.Max(needed, 4), out needed), "GetTokenInformation(TokenCapabilities)");
                        int count = Marshal.ReadInt32(groups);
                        string[] capabilities = Enumerable.Range(0, count).Select(i => "unexpected-capability-" + i).ToArray();
                        return Tuple.Create(isAppContainer, tokenSid, capabilities);
                    } finally { Marshal.FreeHGlobal(groups); }
                } finally { Marshal.FreeHGlobal(booleanBuffer); }
            } finally { CloseHandle(token); }
        }

        private static int[] JobPids(IntPtr job) {
            const int bytes = 65536;
            IntPtr buffer = Marshal.AllocHGlobal(bytes);
            try {
                uint returned;
                Win32(QueryInformationJobObject(job, 3, buffer, bytes, out returned), "QueryInformationJobObject");
                int count = Marshal.ReadInt32(buffer, 4);
                int offset = 8;
                int pointerSize = IntPtr.Size;
                List<int> result = new List<int>();
                for (int index = 0; index < count; index++) {
                    long value = pointerSize == 8 ? Marshal.ReadInt64(buffer, offset + (index * pointerSize)) : Marshal.ReadInt32(buffer, offset + (index * pointerSize));
                    if (value > 0 && value <= Int32.MaxValue) result.Add((int)value);
                }
                return result.ToArray();
            } finally { Marshal.FreeHGlobal(buffer); }
        }

        private static long PeakJobMemory(IntPtr job) {
            int size = Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION));
            IntPtr buffer = Marshal.AllocHGlobal(size);
            try {
                uint returned;
                Win32(QueryInformationJobObject(job, 9, buffer, (uint)size, out returned), "QueryInformationJobObject(ExtendedLimitInformation)");
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION value = (JOBOBJECT_EXTENDED_LIMIT_INFORMATION)Marshal.PtrToStructure(buffer, typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION));
                return unchecked((long)value.PeakJobMemoryUsed.ToUInt64());
            } finally { Marshal.FreeHGlobal(buffer); }
        }

        private static int TotalAssignedProcesses(IntPtr job) {
            int size = Marshal.SizeOf(typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION));
            IntPtr buffer = Marshal.AllocHGlobal(size);
            try {
                uint returned;
                Win32(QueryInformationJobObject(job, 1, buffer, (uint)size, out returned), "QueryInformationJobObject(BasicAccountingInformation)");
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION value = (JOBOBJECT_BASIC_ACCOUNTING_INFORMATION)Marshal.PtrToStructure(buffer, typeof(JOBOBJECT_BASIC_ACCOUNTING_INFORMATION));
                return checked((int)value.TotalProcesses);
            } finally { Marshal.FreeHGlobal(buffer); }
        }

        private static string[] NetworkLines(IEnumerable<int> pids) {
            HashSet<int> owned = new HashSet<int>(pids);
            if (owned.Count == 0) return new string[0];
            ProcessStartInfo start = new ProcessStartInfo(Environment.ExpandEnvironmentVariables(@"%SystemRoot%\System32\netstat.exe"), "-ano");
            start.UseShellExecute = false;
            start.CreateNoWindow = true;
            start.RedirectStandardOutput = true;
            using (Process process = Process.Start(start)) {
                string output = process.StandardOutput.ReadToEnd();
                process.WaitForExit(5000);
                return output.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries)
                    .Where(line => {
                        Match match = Regex.Match(line, @"\s(\d+)\s*$");
                        return match.Success && owned.Contains(Int32.Parse(match.Groups[1].Value));
                    }).Where(line => line.TrimStart().StartsWith("TCP ", StringComparison.OrdinalIgnoreCase) || line.TrimStart().StartsWith("UDP ", StringComparison.OrdinalIgnoreCase))
                    .Select(line => line.Trim()).ToArray();
            }
        }

        public static SandboxEvidence Run(string profileName, string executable, string commandLine, string currentDirectory, int timeoutMilliseconds, int activeProcessLimit, long aggregateMemoryLimitBytes) {
            IntPtr sid = IntPtr.Zero;
            IntPtr attributeList = IntPtr.Zero;
            IntPtr securityCapabilitiesBuffer = IntPtr.Zero;
            IntPtr job = IntPtr.Zero;
            PROCESS_INFORMATION process = new PROCESS_INFORMATION();
            bool profileCreated = false;
            SandboxEvidence evidence = new SandboxEvidence {
                profileName = profileName,
                capabilities = new string[0],
                processes = new ProcessRecord[0],
                loadedModules = new ModuleRecord[0],
                moduleInventoryComplete = true,
                networkConnections = new string[0],
                activeProcessLimit = activeProcessLimit,
                aggregateMemoryLimitBytes = aggregateMemoryLimitBytes,
                breakawayAllowed = false,
            };
            HashSet<int> observedPids = new HashSet<int>();
            Dictionary<int, ProcessRecord> observedProcesses = new Dictionary<int, ProcessRecord>();
            Dictionary<string, ModuleRecord> observedModules = new Dictionary<string, ModuleRecord>(StringComparer.OrdinalIgnoreCase);
            HashSet<string> network = new HashSet<string>(StringComparer.Ordinal);
            Exception failure = null;
            try {
                int profileResult = CreateAppContainerProfile(profileName, "Document Studio LibreOffice proof", "Proof-only zero-capability Office runtime", IntPtr.Zero, 0, out sid);
                if (profileResult < 0) throw new InvalidOperationException("CreateAppContainerProfile HRESULT 0x" + profileResult.ToString("X8"));
                profileCreated = true;
                evidence.appContainerSid = SidText(sid);

                IntPtr attributeSize = IntPtr.Zero;
                InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref attributeSize);
                attributeList = Marshal.AllocHGlobal(attributeSize);
                Win32(InitializeProcThreadAttributeList(attributeList, 1, 0, ref attributeSize), "InitializeProcThreadAttributeList");
                SECURITY_CAPABILITIES securityCapabilities = new SECURITY_CAPABILITIES { AppContainerSid = sid, Capabilities = IntPtr.Zero, CapabilityCount = 0, Reserved = 0 };
                securityCapabilitiesBuffer = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(SECURITY_CAPABILITIES)));
                Marshal.StructureToPtr(securityCapabilities, securityCapabilitiesBuffer, false);
                Win32(UpdateProcThreadAttribute(attributeList, 0, new IntPtr(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES), securityCapabilitiesBuffer, new IntPtr(Marshal.SizeOf(typeof(SECURITY_CAPABILITIES))), IntPtr.Zero, IntPtr.Zero), "UpdateProcThreadAttribute");

                job = CreateJobObject(IntPtr.Zero, null);
                if (job == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateJobObject");
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_JOB_MEMORY;
                limits.BasicLimitInformation.ActiveProcessLimit = (uint)activeProcessLimit;
                limits.JobMemoryLimit = new UIntPtr((ulong)aggregateMemoryLimitBytes);
                IntPtr limitsBuffer = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION)));
                try {
                    Marshal.StructureToPtr(limits, limitsBuffer, false);
                    Win32(SetInformationJobObject(job, 9, limitsBuffer, (uint)Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION))), "SetInformationJobObject");
                } finally { Marshal.FreeHGlobal(limitsBuffer); }

                STARTUPINFOEX startup = new STARTUPINFOEX();
                startup.StartupInfo.cb = Marshal.SizeOf(typeof(STARTUPINFOEX));
                startup.lpAttributeList = attributeList;
                Win32(CreateProcess(executable, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero, false, CREATE_SUSPENDED | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT, IntPtr.Zero, currentDirectory, ref startup, out process), "CreateProcessW");
                evidence.rootPid = process.dwProcessId;
                Win32(AssignProcessToJobObject(job, process.hProcess), "AssignProcessToJobObject");
                evidence.assignedBeforeResume = true;
                Tuple<bool, string, string[]> token = TokenEvidence(process.hProcess);
                evidence.appContainer = token.Item1;
                evidence.tokenAppContainerSid = token.Item2;
                evidence.capabilities = token.Item3;
                if (!evidence.appContainer || evidence.tokenAppContainerSid != evidence.appContainerSid || evidence.capabilities.Length != 0) {
                    throw new InvalidOperationException("AppContainer token evidence mismatch");
                }
                if (ResumeThread(process.hThread) == UInt32.MaxValue) throw new Win32Exception(Marshal.GetLastWin32Error(), "ResumeThread");

                Stopwatch clock = Stopwatch.StartNew();
                uint wait = WAIT_TIMEOUT;
                bool rootExited = false;
                bool jobDrained = false;
                while (clock.ElapsedMilliseconds < timeoutMilliseconds) {
                    int[] pids = JobPids(job);
                    evidence.peakAssignedProcessCount = Math.Max(evidence.peakAssignedProcessCount, pids.Length);
                    foreach (int pid in pids) {
                        observedPids.Add(pid);
                        try {
                            using (Process owned = Process.GetProcessById(pid)) {
                                if (!observedProcesses.ContainsKey(pid)) {
                                    string path = null;
                                    try { path = owned.MainModule.FileName; } catch { }
                                    observedProcesses[pid] = new ProcessRecord { pid = pid, path = path, name = owned.ProcessName };
                                }
                                try {
                                    foreach (ProcessModule module in owned.Modules) {
                                        string modulePath = module.FileName;
                                        if (!String.IsNullOrWhiteSpace(modulePath)) observedModules[pid + "|" + modulePath] = new ModuleRecord { pid = pid, path = modulePath };
                                    }
                                } catch { evidence.moduleInventoryComplete = false; }
                            }
                        } catch { evidence.moduleInventoryComplete = false; }
                    }
                    foreach (string line in NetworkLines(pids)) network.Add(line);
                    wait = WaitForSingleObject(process.hProcess, 100);
                    if (wait == WAIT_OBJECT_0) rootExited = true;
                    if (wait != WAIT_TIMEOUT && wait != WAIT_OBJECT_0) throw new Win32Exception(Marshal.GetLastWin32Error(), "WaitForSingleObject");
                    if (rootExited && pids.Length == 0) { jobDrained = true; break; }
                }
                if (!jobDrained) {
                    evidence.timedOut = true;
                    Win32(TerminateJobObject(job, 0xD5040001), "TerminateJobObject(timeout)");
                    WaitForSingleObject(process.hProcess, 5000);
                }
                uint exitCode;
                Win32(GetExitCodeProcess(process.hProcess, out exitCode), "GetExitCodeProcess");
                evidence.exitCode = unchecked((int)exitCode);
                if (!observedProcesses.ContainsKey(process.dwProcessId)) {
                    observedProcesses[process.dwProcessId] = new ProcessRecord { pid = process.dwProcessId, path = executable, name = Path.GetFileName(executable) };
                }
                evidence.processes = observedProcesses.Values.OrderBy(value => value.pid).ToArray();
                evidence.loadedModules = observedModules.Values.OrderBy(value => value.pid).ThenBy(value => value.path, StringComparer.OrdinalIgnoreCase).ToArray();
                evidence.networkConnections = network.OrderBy(value => value, StringComparer.Ordinal).ToArray();
                evidence.totalAssignedProcesses = TotalAssignedProcesses(job);
                evidence.peakJobMemoryBytes = PeakJobMemory(job);
            }
            catch (Exception caught) {
                failure = caught;
                if (process.dwProcessId != 0 && !observedProcesses.ContainsKey(process.dwProcessId)) {
                    observedProcesses[process.dwProcessId] = new ProcessRecord { pid = process.dwProcessId, path = executable, name = Path.GetFileName(executable) };
                }
                if (job != IntPtr.Zero) {
                    try {
                        foreach (int pid in JobPids(job)) {
                            observedPids.Add(pid);
                            if (!observedProcesses.ContainsKey(pid)) {
                                string path = null;
                                string name = null;
                                try {
                                    using (Process owned = Process.GetProcessById(pid)) {
                                        name = owned.ProcessName;
                                        try { path = owned.MainModule.FileName; } catch { }
                                    }
                                } catch { }
                                observedProcesses[pid] = new ProcessRecord { pid = pid, path = path, name = name };
                            }
                        }
                        foreach (string line in NetworkLines(observedPids)) network.Add(line);
                        evidence.totalAssignedProcesses = TotalAssignedProcesses(job);
                        evidence.peakJobMemoryBytes = PeakJobMemory(job);
                    } catch { }
                    try { TerminateJobObject(job, 0xD5040002); } catch { }
                }
                evidence.processes = observedProcesses.Values.OrderBy(value => value.pid).ToArray();
                evidence.loadedModules = observedModules.Values.OrderBy(value => value.pid).ThenBy(value => value.path, StringComparer.OrdinalIgnoreCase).ToArray();
                evidence.networkConnections = network.OrderBy(value => value, StringComparer.Ordinal).ToArray();
            }
            finally {
                if (job != IntPtr.Zero) CloseHandle(job);
                if (process.hThread != IntPtr.Zero) CloseHandle(process.hThread);
                if (process.hProcess != IntPtr.Zero) CloseHandle(process.hProcess);
                if (attributeList != IntPtr.Zero) { DeleteProcThreadAttributeList(attributeList); Marshal.FreeHGlobal(attributeList); }
                if (securityCapabilitiesBuffer != IntPtr.Zero) Marshal.FreeHGlobal(securityCapabilitiesBuffer);
                if (sid != IntPtr.Zero) FreeSid(sid);
                if (profileCreated) evidence.profileDeleted = DeleteAppContainerProfile(profileName) >= 0;
            }
            if (failure != null) throw new SandboxRunException("Office sandbox failed: " + failure.Message, failure, evidence);
            return evidence;
        }
    }
}
