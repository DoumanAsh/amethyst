use std::error::Error;

use futures::{Async, Poll};
use futures::future::{Future, IntoFuture, Shared, SharedItem, SharedError};
use rayon::ThreadPool;
use specs::{Component, DenseVecStorage};

use {BoxedErr, SharedAssetError, StoreId};

/// One of the three core traits of this crate.
///
/// You want to implement this for every type of asset like
///
/// * `Mesh`
/// * `Texture`
/// * `Terrain`
///
/// and so on. Now, an asset may be available in different formats.
/// That's why we have the `Data` associated type here. You can specify
/// an intermediate format here, like the vertex data for a mesh or the samples
/// for audio data.
///
/// This data is then generated by the `Format` trait.
pub trait Asset: Sized {
    /// The `Context` type that can produce this asset
    type Context: Context<Asset = Self>;
}

/// A future for an asset
pub struct AssetFuture<A>(pub Shared<Box<Future<Item = A, Error = BoxedErr>>>);

impl<A> AssetFuture<A> {
    /// Wrap another future into `AssetFuture`
    pub fn from_future<F>(f: F) -> Self
    where
        F: IntoFuture<Item = A, Error = BoxedErr> + 'static,
    {
        let f: Box<Future<Item = A, Error = BoxedErr>> = Box::new(f.into_future());
        AssetFuture(f.shared())
    }
}

impl<A> Component for AssetFuture<A>
where
    A: Component,
    Self: 'static,
{
    type Storage = DenseVecStorage<Self>;
}

impl<A> AssetFuture<A> {
    /// If any clone of this future has completed execution, returns its result immediately
    /// without blocking.
    /// Otherwise, returns None without triggering the work represented by this future.
    pub fn peek(&self) -> Option<Result<SharedItem<A>, SharedError<BoxedErr>>> {
        self.0.peek()
    }
}

impl<A> Clone for AssetFuture<A> {
    fn clone(&self) -> Self {
        AssetFuture(self.0.clone())
    }
}

impl<A> Future for AssetFuture<A>
where
    A: Clone,
{
    type Item = A;
    type Error = BoxedErr;

    fn poll(&mut self) -> Poll<A, BoxedErr> {
        match self.0.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(asset)) => Ok(Async::Ready((*asset).clone())),
            Err(err) => Err(BoxedErr(Box::new(SharedAssetError::from(err)))),
        }
    }
}
impl<A> From<Shared<Box<Future<Item = A, Error = BoxedErr>>>> for AssetFuture<A> {
    fn from(inner: Shared<Box<Future<Item = A, Error = BoxedErr>>>) -> Self {
        AssetFuture(inner)
    }
}

/// A specifier for an asset, uniquely identifying it by
///
/// * the extension (the format it was provided in)
/// * its name
/// * the storage it was loaded from
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AssetSpec {
    /// The possible extensions of this asset
    pub exts: &'static [&'static str],
    /// The name of this asset.
    pub name: String,
    /// Unique identifier indicating the Storage from which the asset was loaded.
    pub store: StoreId,
}

impl AssetSpec {
    /// Creates a new asset specifier from the given parameters.
    pub fn new(name: String, exts: &'static [&'static str], store: StoreId) -> Self {
        AssetSpec { exts, name, store }
    }
}

/// The context type which manages assets of one type.
/// It is responsible for caching
pub trait Context: Send + Sync + 'static {
    /// The asset type this context can produce.
    type Asset: Asset;
    /// The `Data` type the asset can be created from.
    type Data;
    /// The error that may be returned from `create_asset`.
    type Error: Error + Send + Sync;
    /// The result type for loading an asset. This can also be a future
    /// (or anything that implements `IntoFuture`).
    type Result: IntoFuture<Item = Self::Asset, Error = Self::Error>;

    /// A small keyword for which category these assets belongs to.
    ///
    /// ## Examples
    ///
    /// * `"mesh"` for `Mesh`
    /// * `"data"` for `Level`
    ///
    /// The storage may use this information, to e.g. search the identically-named
    /// subfolder.
    fn category(&self) -> &str;

    /// Provides the conversion from the data format to the actual asset.
    fn create_asset(&self, data: Self::Data, pool: &ThreadPool) -> Self::Result;

    /// Notifies about an asset load. This is can be used to cache the asset.
    /// To return a cached asset, see the `retrieve` function.
    fn cache(&self, _spec: AssetSpec, _asset: AssetFuture<Self::Asset>) {}

    /// Returns `Some` cached value if possible, otherwise `None`.
    ///
    /// For a basic implementation of a cache, please take a look at the `Cache` type.
    fn retrieve(&self, _spec: &AssetSpec) -> Option<AssetFuture<Self::Asset>> {
        None
    }

    /// Updates an asset after it's been reloaded.
    ///
    /// This usually just puts the new asset into a queue;
    /// the actual update happens by calling `update` on the
    /// asset.
    fn update(&self, spec: &AssetSpec, asset: AssetFuture<Self::Asset>);

    /// Gives a hint that several assets may have been released recently.
    ///
    /// This is useful if your assets are reference counted, because you are
    /// now able to remove unique assets from the cache, leaving the shared
    /// ones there.
    fn clear(&self) {}

    /// Request for clearing the whole cache.
    fn clear_all(&self) {}
}

/// A format, providing a conversion from bytes to asset data, which is then
/// in turn accepted by `Asset::from_data`. Examples for formats are
/// `Png`, `Obj` and `Wave`.
pub trait Format {
    /// A list of the extensions (without `.`).
    ///
    /// ## Examples
    ///
    /// * `"png"`
    /// * `"obj"`
    /// * `"wav"`
    const EXTENSIONS: &'static [&'static str];
    /// The data type this format is able to load.
    type Data;
    /// The error that may be returned from `Format::parse`.
    type Error: Error + Send + Sync;
    /// The result of the `parse` method. Can be anything that implements
    /// `IntoFuture`.
    type Result: IntoFuture<Item = Self::Data, Error = Self::Error>;



    /// Reads the given bytes and produces asset data.
    fn parse(&self, bytes: Vec<u8>, pool: &ThreadPool) -> Self::Result;
}
