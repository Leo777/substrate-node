#![cfg_attr(not(feature = "std"), no_std)]
#![allow(unused_imports)]
#![allow(dead_code)]
use core::{convert::TryInto, fmt};
use frame_support::sp_runtime::offchain::storage::{
	MutateStorageError, StorageRetrievalError, StorageValueRef,
};
use serde::{Deserialize, Deserializer};

use parity_scale_codec::{Decode, Encode};
use sp_io::offchain_index;

use frame_support::sp_runtime::{
	offchain as rt_offchain,
	offchain::storage_lock::{BlockAndTime, StorageLock},
	traits::BlockNumberProvider,
};

use frame_support::sp_std::{prelude::*, str};
/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://docs.substrate.io/v3/runtime/frame>
pub use pallet::*;

#[derive(Debug, Deserialize, Encode, Decode, Default)]
struct Fetching(Vec<u8>, bool);

const ONCHAIN_TX_KEY: &[u8] = b"ocw-demo::storage::tx";

// const HTTP_REMOTE_REQUEST: &str =
// 	"http://www.randomnumberapi.com/api/v1.0/random?min=100&max=1000&count=5";
const HTTP_REMOTE_REQUEST: &str = "https://hacker-news.firebaseio.com/v0/item/9129911.json";

const FETCH_TIMEOUT_PERIOD: u64 = 3000; // in milli-seconds
const LOCK_TIMEOUT_EXPIRATION: u64 = FETCH_TIMEOUT_PERIOD + 1000; // in milli-seconds
const LOCK_BLOCK_EXPIRATION: u32 = 3; // in block number

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;
#[frame_support::pallet]
pub mod pallet {
	use super::*;
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_authorship::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>>
			+ IsType<<Self as frame_system::Config>::Event>
			+ TryInto<Event<Self>>;

		type Authorship: pallet_authorship::Config;

		#[pallet::constant]
		type GracePeriod: Get<Self::BlockNumber>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	// The pallet's runtime storage items.
	// https://docs.substrate.io/v3/runtime/storage
	#[pallet::storage]
	#[pallet::getter(fn something)]
	// Learn more about declaring storage items:
	// https://docs.substrate.io/v3/runtime/storage#declaring-storage-items
	pub type Something<T> = StorageValue<_, u32>;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [something, who]
		SomethingStored(u32, T::AccountId),
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Error names should be descriptive.
		NoneValue,
		/// Errors should have helpful documentation associated with them.
		StorageOverflow,
		HttpFetchingError,
	}

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// An example dispatchable that takes a singles value as a parameter, writes the value to
		/// storage and emits an event. This function must be dispatched by a signed extrinsic.
		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn do_something(origin: OriginFor<T>, something: u32) -> DispatchResult {
			// Check that the extrinsic was signed and get the signer.
			// This function will return an error if the extrinsic is not signed.
			// https://docs.substrate.io/v3/runtime/origins
			let who = ensure_signed(origin)?;

			// Update storage.
			<Something<T>>::put(something);

			let key = Self::derived_key(frame_system::Pallet::<T>::block_number());
			let data = Fetching(b"fetching".to_vec(), true);
			offchain_index::set(&key, &data.encode());

			// Emit an event.
			Self::deposit_event(Event::SomethingStored(something, who));
			// Return a successful DispatchResultWithPostInfo
			Ok(())
		}

		/// An example dispatchable that may throw a custom error.
		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn cause_error(origin: OriginFor<T>) -> DispatchResult {
			let _who = ensure_signed(origin)?;

			// Read a value from storage.
			match <Something<T>>::get() {
				// Return an error if the value has not been set.
				None => return Err(Error::<T>::NoneValue.into()),
				Some(old) => {
					// Increment the value read from storage; will error in the event of overflow.
					let new = old.checked_add(1).ok_or(Error::<T>::StorageOverflow)?;
					// Update the value in storage with the incremented result.
					<Something<T>>::put(new);
					Ok(())
				},
			}
		}
	}
	impl<T: Config> BlockNumberProvider for Pallet<T> {
		type BlockNumber = T::BlockNumber;

		fn current_block_number() -> Self::BlockNumber {
			<frame_system::Pallet<T>>::block_number()
		}
	}

	impl<T: Config> Pallet<T> {
		#[deny(clippy::clone_double_ref)]
		fn derived_key(block_number: T::BlockNumber) -> Vec<u8> {
			block_number.using_encoded(|encoded_bn| {
				ONCHAIN_TX_KEY
					.iter()
					.chain(b"/".iter())
					.chain(encoded_bn)
					.copied()
					.collect::<Vec<u8>>()
			})
		}

		/// This function uses the `offchain::http` API to query the remote endpoint information,
		///   and returns the JSON response as vector of bytes.
		fn fetch_from_remote() -> Result<Vec<u8>, Error<T>> {
			// Initiate an external HTTP GET request. This is using high-level wrappers from
			// `sp_runtime`.
			let request = rt_offchain::http::Request::get(HTTP_REMOTE_REQUEST);

			// Keeping the offchain worker execution time reasonable, so limiting the call to be
			// within 3s.
			let timeout = sp_io::offchain::timestamp()
				.add(rt_offchain::Duration::from_millis(FETCH_TIMEOUT_PERIOD));

			let pending = request
				.deadline(timeout) // Setting the timeout time
				.send() // Sending the request out by the host
				.map_err(|e| {
					log::error!("{:?}", e);
					<Error<T>>::HttpFetchingError
				})?;

			// By default, the http request is async from the runtime perspective. So we are asking
			// the   runtime to wait here
			// The returning value here is a `Result` of `Result`, so we are unwrapping it twice by
			// two `?`   ref: https://docs.substrate.io/rustdocs/latest/sp_runtime/offchain/http/struct.PendingRequest.html#method.try_wait
			let response = pending
				.try_wait(timeout)
				.map_err(|e| {
					log::error!("{:?}", e);
					<Error<T>>::HttpFetchingError
				})?
				.map_err(|e| {
					log::error!("{:?}", e);
					<Error<T>>::HttpFetchingError
				})?;

			if response.code != 200 {
				log::error!("Unexpected http request status code: {}", response.code);
				return Err(<Error<T>>::HttpFetchingError)
			}

			// Next we fully read the response body and collect it to a vector of bytes.
			Ok(response.body().collect::<Vec<u8>>())
		}
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn offchain_worker(block_number: T::BlockNumber) {
			log::info!("OCW {:?}", block_number);

			<pallet_authorship::Pallet<T>>::author();
		// 	// Reading back the off-chain indexing value. It is exactly the same as reading from
		// 	// ocw local storage.
		// 	let key = Self::derived_key(block_number);
		// 	let oci_mem = StorageValueRef::persistent(&key);

		// 	let mut lock = StorageLock::<BlockAndTime<Self>>::with_block_and_time_deadline(
		// 		b"ocw-demo::lock",
		// 		LOCK_BLOCK_EXPIRATION,
		// 		rt_offchain::Duration::from_millis(LOCK_TIMEOUT_EXPIRATION),
		// 	);
		// 	if let Ok(_guard) = lock.try_lock() {
		// 		if let Ok(Some(data)) = oci_mem.get::<Fetching>() {
		// 			log::info!("Making HTTTP Call {:?}", data);
		// 		} else {
		// 			log::info!("Chilling");
		// 		}
		// 	} else {
		// 		log::info!("Lock");
		// 	};
		}
	}
}
