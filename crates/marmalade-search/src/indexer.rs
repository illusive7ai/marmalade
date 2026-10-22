//! The Windows Search index, queried through the WinRT `StorageFolder` query API.
//!
//! No admin rights, no service of our own, and it answers from the index the OS already
//! maintains. Runs on the calling thread and blocks, so call it from a worker thread, never the UI.

use windows::core::HSTRING;
use windows::Foundation::Collections::IVector;
use windows::Storage::Search::{FolderDepth, IndexerOption, QueryOptions, SortEntry};
use windows::Storage::StorageFolder;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use crate::{Hit, Request};

/// Query the index under `root`. `aqs` comes from `aqs::build_aqs`.
pub fn query(root: &str, aqs: &str, req: &Request, fetch: u32) -> windows::core::Result<Vec<Hit>> {
    // Ignore the result: it is an error only if this thread is already initialised differently.
    // SAFETY: plain COM/WinRT initialisation on the current thread.
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };

    let folder = StorageFolder::GetFolderFromPathAsync(&HSTRING::from(root))?.get()?;

    let opts = QueryOptions::new()?;
    let types: IVector<HSTRING> = opts.FileTypeFilter()?;
    match req.kind {
        Some(kind) => {
            for ext in kind.extensions() {
                types.Append(&HSTRING::from(format!(".{ext}")))?;
            }
        }
        None => types.Append(&HSTRING::from("*"))?,
    }
    opts.SetFolderDepth(FolderDepth::Deep)?;
    opts.SetIndexerOption(IndexerOption::OnlyUseIndexer)?;
    if !aqs.is_empty() {
        opts.SetUserSearchFilter(&HSTRING::from(aqs))?;
    }
    // Newest first, so the best-ranked hits are not lost when the result is capped.
    let sort: IVector<SortEntry> = opts.SortOrder()?;
    sort.Append(&SortEntry {
        PropertyName: HSTRING::from("System.DateModified"),
        AscendingOrder: false,
    })?;

    // Measured: ~340 ms for a substring name query over a full profile, almost all of it inside
    // GetFilesAsync, and independent of sorting and of how many results are fetched.
    let files = folder
        .CreateFileQueryWithOptions(&opts)?
        .GetFilesAsync(0, fetch)?
        .get()?;
    Ok(files
        .into_iter()
        .filter_map(|f| f.Path().ok())
        .map(|p| Hit {
            path: p.to_string().into(),
            modified: None,
        })
        .collect())
}
