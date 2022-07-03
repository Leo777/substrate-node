#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://docs.substrate.io/v3/runtime/frame>
pub use pallet::*;
use sp_runtime::{
	offchain::{
		http,
		storage::{MutateStorageError, StorageRetrievalError, StorageValueRef},
		Duration,
	},
	traits::Zero,
	transaction_validity::{InvalidTransaction, TransactionValidity, ValidTransaction},
	RuntimeDebug,
};

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;
	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>>
			+ IsType<<Self as frame_system::Config>::Event>
			+ TryInto<Event<Self>>;
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

	enum TransactionType {
		Signed
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Error names should be descriptive.
		NoneValue,
		/// Errors should have helpful documentation associated with them.
		StorageOverflow,
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

	impl<T: Config> Pallet<T> {
		fn choose_transaction_type(block_number: T::BlockNumber) -> TransactionType {
			/// A friendlier name for the error that is going to be returned in case we are in the grace
			/// period.
			const RECENTLY_SENT: () = ();
	
			// Start off by creating a reference to Local Storage value.
			// Since the local storage is common for all offchain workers, it's a good practice
			// to prepend your entry with the module name.
			let val = StorageValueRef::persistent(b"example_ocw::last_send");
			// The Local Storage is persisted and shared between runs of the offchain workers,
			// and offchain workers may run concurrently. We can use the `mutate` function, to
			// write a storage entry in an atomic fashion. Under the hood it uses `compare_and_set`
			// low-level method of local storage API, which means that only one worker
			// will be able to "acquire a lock" and send a transaction if multiple workers
			// happen to be executed concurrently.
			let res = val.mutate(|last_send: Result<Option<T::BlockNumber>, StorageRetrievalError>| {
				match last_send {
					// If we already have a value in storage and the block number is recent enough
					// we avoid sending another transaction at this time.
					Ok(Some(block)) if block_number < block + T::GracePeriod::get() =>
						Err(RECENTLY_SENT),
					// In every other case we attempt to acquire the lock and send a transaction.
					_ => Ok(block_number),
				}
			});
	
			// The result of `mutate` call will give us a nested `Result` type.
			// The first one matches the return of the closure passed to `mutate`, i.e.
			// if we return `Err` from the closure, we get an `Err` here.
			// In case we return `Ok`, here we will have another (inner) `Result` that indicates
			// if the value has been set to the storage correctly - i.e. if it wasn't
			// written to in the meantime.
			match res {
				// The value has been set correctly, which means we can safely send a transaction now.
				Ok(block_number) => {
					// Depending if the block is even or odd we will send a `Signed` or `Unsigned`
					// transaction.
					// Note that this logic doesn't really guarantee that the transactions will be sent
					// in an alternating fashion (i.e. fairly distributed). Depending on the execution
					// order and lock acquisition, we may end up for instance sending two `Signed`
					// transactions in a row. If a strict order is desired, it's better to use
					// the storage entry for that. (for instance store both block number and a flag
					// indicating the type of next transaction to send).
					
						TransactionType::Signed
					
				},
				// We are in the grace period, we should not send a transaction this time.
				Err(MutateStorageError::ValueFunctionFailed(RECENTLY_SENT)) => TransactionType::None,
				// We wanted to send a transaction, but failed to write the block number (acquire a
				// lock). This indicates that another offchain worker that was running concurrently
				// most likely executed the same logic and succeeded at writing to storage.
				// Thus we don't really want to send the transaction, knowing that the other run
				// already did.
				Err(MutateStorageError::ConcurrentModification(_)) => TransactionType::None,
			}
		}	
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
		fn offchain_worker(block_number: T::BlockNumber) {
			log::info!("OCW {:?}", block_number);
			if <frame_system::Pallet<T>>::read_events_no_consensus()
				.into_iter()
				.filter_map(|event_record| {
					let local_event = <T as Config>::Event::from(event_record.event);
					local_event.try_into().ok()
				})
				.any(|event| matches!(event, Event::SomethingStored(..)))
			{
				let should_send = Self::choose_transaction_type(block_number);
			let res = match should_send {
				TransactionType::Signed => Self::fetch_price_and_send_signed(),
				
			};
			if let Err(e) = res {
				log::error!("Error: {}", e);
			}
			log::info!("Hello world from OCW {:?}", block_number);
			}
		}
	}
}
