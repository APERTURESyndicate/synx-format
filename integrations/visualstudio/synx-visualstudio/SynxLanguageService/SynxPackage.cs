using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using Microsoft.VisualStudio;
using Microsoft.VisualStudio.OLE.Interop;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio.Shell.Interop;
using SynxLanguageService.Commands;
using SynxLanguageService.Formatting;
using Task = System.Threading.Tasks.Task;

namespace SynxLanguageService
{
    [PackageRegistration(UseManagedResourcesOnly = true, AllowsBackgroundLoading = true)]
    [Guid(PackageGuidString)]
    [ProvideMenuResource("Menus.ctmenu", 1)]
    [ProvideAutoLoad(VSConstants.UICONTEXT.SolutionExistsAndFullyLoaded_string, PackageAutoLoadFlags.BackgroundLoad)]
    public sealed class SynxPackage : AsyncPackage
    {
        public const string PackageGuidString = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

        public static readonly Guid CommandSet = new("b2c3d4e5-f6a7-8901-bcde-f12345678901");
        public const int ConvertToJsonCommandId = 0x0100;
        public const int ConvertFromJsonCommandId = 0x0101;
        public const int FreezeCommandId = 0x0102;
        public const int FormatCommandId = 0x0103;

        protected override async Task InitializeAsync(CancellationToken cancellationToken, IProgress<ServiceProgressData> progress)
        {
            await JoinableTaskFactory.SwitchToMainThreadAsync(cancellationToken);
            // Commands are handled via the menu resource (Menus.ctmenu)
            // The actual command implementations are in SynxCommands.cs
        }

        /// <summary>
        /// Execute Convert to JSON on the active document.
        /// </summary>
        public static void ExecuteConvertToJson()
        {
            ThreadHelper.ThrowIfNotOnUIThread();
            var dte = Package.GetGlobalService(typeof(EnvDTE.DTE)) as EnvDTE.DTE;
            if (dte?.ActiveDocument == null) return;

            var doc = dte.ActiveDocument;
            var textDoc = doc.Object("TextDocument") as EnvDTE.TextDocument;
            if (textDoc == null) return;

            var editPoint = textDoc.StartPoint.CreateEditPoint();
            var text = editPoint.GetText(textDoc.EndPoint);

            if (!doc.FullName.EndsWith(".synx", StringComparison.OrdinalIgnoreCase)) return;

            var json = SynxCommands.ConvertToJson(text);
            var jsonPath = Path.ChangeExtension(doc.FullName, ".json");
            File.WriteAllText(jsonPath, json, System.Text.Encoding.UTF8);
            dte.ItemOperations.OpenFile(jsonPath);
        }

        /// <summary>
        /// Execute Convert JSON to SYNX on the active document.
        /// </summary>
        public static void ExecuteConvertFromJson()
        {
            ThreadHelper.ThrowIfNotOnUIThread();
            var dte = Package.GetGlobalService(typeof(EnvDTE.DTE)) as EnvDTE.DTE;
            if (dte?.ActiveDocument == null) return;

            var doc = dte.ActiveDocument;
            var textDoc = doc.Object("TextDocument") as EnvDTE.TextDocument;
            if (textDoc == null) return;

            var editPoint = textDoc.StartPoint.CreateEditPoint();
            var text = editPoint.GetText(textDoc.EndPoint);

            if (!doc.FullName.EndsWith(".json", StringComparison.OrdinalIgnoreCase)) return;

            var synx = SynxCommands.ConvertFromJson(text);
            var synxPath = Path.ChangeExtension(doc.FullName, ".synx");
            File.WriteAllText(synxPath, synx, System.Text.Encoding.UTF8);
            dte.ItemOperations.OpenFile(synxPath);
        }

        /// <summary>
        /// Execute Freeze: resolve all markers and save .static.synx.
        /// </summary>
        public static void ExecuteFreeze()
        {
            ThreadHelper.ThrowIfNotOnUIThread();
            var dte = Package.GetGlobalService(typeof(EnvDTE.DTE)) as EnvDTE.DTE;
            if (dte?.ActiveDocument == null) return;

            var doc = dte.ActiveDocument;
            var textDoc = doc.Object("TextDocument") as EnvDTE.TextDocument;
            if (textDoc == null) return;

            var editPoint = textDoc.StartPoint.CreateEditPoint();
            var text = editPoint.GetText(textDoc.EndPoint);

            if (!doc.FullName.EndsWith(".synx", StringComparison.OrdinalIgnoreCase)) return;

            var frozen = SynxCommands.Freeze(text);
            var frozenPath = doc.FullName.Replace(".synx", ".static.synx");
            File.WriteAllText(frozenPath, frozen, System.Text.Encoding.UTF8);
            dte.ItemOperations.OpenFile(frozenPath);
        }

        /// <summary>
        /// Execute Format Document.
        /// </summary>
        public static void ExecuteFormat()
        {
            ThreadHelper.ThrowIfNotOnUIThread();
            var dte = Package.GetGlobalService(typeof(EnvDTE.DTE)) as EnvDTE.DTE;
            if (dte?.ActiveDocument == null) return;

            var doc = dte.ActiveDocument;
            var textDoc = doc.Object("TextDocument") as EnvDTE.TextDocument;
            if (textDoc == null) return;

            var editPoint = textDoc.StartPoint.CreateEditPoint();
            var text = editPoint.GetText(textDoc.EndPoint);

            if (!doc.FullName.EndsWith(".synx", StringComparison.OrdinalIgnoreCase)) return;

            var formatted = SynxFormatter.Format(text);
            editPoint.ReplaceText(textDoc.EndPoint, formatted, 0);
        }
    }
}
