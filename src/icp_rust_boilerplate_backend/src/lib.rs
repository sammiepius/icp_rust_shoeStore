#[macro_use]
extern crate serde;
use candid::{Decode, Encode};
use ic_cdk::api::time;
use candid::Principal;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BoundedStorable, Cell, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::{borrow::Cow, cell::RefCell};
use ic_cdk::caller;

type Memory = VirtualMemory<DefaultMemoryImpl>;
type IdCell = Cell<u64, Memory>;

#[derive(candid::CandidType, Clone, Serialize, Deserialize, Default)]
struct Shoe {
    owner: String,
    id: u64,
    name: String,
    size: String,
    shoe_url: String,
    price: i16,
    quantity: i16,
    like: u32,
    liked_by: Vec<Principal>,
    created_at: u64,
    updated_at: Option<u64>,
}

// a trait that must be implemented for a struct that is stored in a stable struct
impl Storable for Shoe {
    fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}
    
// another trait that must be implemented for a struct that is stored in a stable struct
impl BoundedStorable for Shoe {
    const MAX_SIZE: u32 = 1024;
    const IS_FIXED_SIZE: bool = false;
}

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );

    static ID_COUNTER: RefCell<IdCell> = RefCell::new(
        IdCell::init(MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))), 0)
            .expect("Cannot create a counter")
    );

    static SHOE_STORAGE: RefCell<StableBTreeMap<u64, Shoe, Memory>> =
        RefCell::new(StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1)))
    ));
}

#[derive(candid::CandidType, Serialize, Deserialize, Default)]
struct ShoeSearchCriteria {
    name: Option<String>,
    size: Option<String>,
    min_price: Option<i16>,
    max_price: Option<i16>,
}


// Shoe payload for adding or updating a Shoes
#[derive(candid::CandidType, Serialize, Deserialize, Default)]
struct ShoePayload {
    name: String,
    size: String,
    shoe_url: String,
    price: i16,
    quantity: i16,
}

use regex::Regex;

fn is_valid_url(url: &str) -> bool {
    let url_regex = Regex::new(r"^(http://www\.|https://www\.|http://|https://)?[a-z0-9]+([\-\.]{1}[a-z0-9]+)*\.[a-z]{2,5}(:[0-9]{1,5})?(/.*)?$").unwrap();
    url_regex.is_match(url)
}

fn is_valid_price(price: i16) -> bool {
    price >= 0 // Assuming the price can't be negative
}

fn is_valid_quantity(quantity: i16) -> bool {
    quantity > 0 // Assuming at least 1 item must be present
}

fn validate_shoe_payload(payload: &ShoePayload) -> Result<(), String> {
    if !is_valid_url(&payload.shoe_url) {
        return Err("Invalid URL format".into());
    }
    if !is_valid_price(payload.price) {
        return Err("Invalid price value".into());
    }
    if !is_valid_quantity(payload.quantity) {
        return Err("Invalid quantity value".into());
    }
    Ok(())
}


#[ic_cdk::update]
fn add_shoe(shoe_payload: ShoePayload) -> Result<Shoe, String> {
    validate_shoe_payload(&shoe_payload)?;

    let liked_by: Vec<Principal> = Vec::new(); // Initializes an empty Vec for the liked field
    let id = ID_COUNTER
        .with(|counter| {
            let current_value = *counter.borrow().get();
            counter.borrow_mut().set(current_value + 1)
        })
        .expect("cannot increment id counter");

    let shoe = Shoe {
        owner: caller().to_string(),
        id,
        name: shoe_payload.name,
        size: shoe_payload.size,
        shoe_url: shoe_payload.shoe_url,
        price: shoe_payload.price,
        quantity: shoe_payload.quantity,
        like: 0,
        liked_by,
        created_at: time(),
        updated_at: None,
    };

    do_insert(&shoe);
    Ok(shoe)
}


// get all the available shoes in the store
#[ic_cdk::query]
fn get_shoes(page: usize, page_size: usize, sort_by: String) -> Result<Vec<Shoe>, String> {
    SHOE_STORAGE.with(|service| {
        let mut shoes: Vec<Shoe> = service.borrow()
                                            .iter()
                                            .map(|(_, shoe)| shoe.clone())
                                            .collect();

        // Sorting logic
        match sort_by.as_str() {
            "price" => shoes.sort_by_key(|shoe| shoe.price),
            "created_at" => shoes.sort_by_key(|shoe| shoe.created_at),
            _ => return Err("Invalid sorting parameter".into()),
        }

        // Pagination logic
        let start = page * page_size;
        let end = start + page_size;
        if start >= shoes.len() {
            return Ok(vec![]);
        }

        Ok(shoes[start..std::cmp::min(end, shoes.len())].to_vec())
    })
}
    
// function to retrieve details of a specific Shoe by the shoe id
#[ic_cdk::query]
fn get_shoe_by_id(id: u64) -> Result<Shoe, Error> {
    match _get_shoe(&id) {
        Some(shoe) => Ok(shoe),
        None => Err(Error::NotFound {
            msg: format!("a shoe with id={} not found", id),
        }),
    }
}

// Get Total number of shoes available in the store
    #[ic_cdk::query]
    fn total_number_of_shoes() -> i16 {
    SHOE_STORAGE.with(|service| {
        let storage = service.borrow_mut();
        storage.iter().map(|(_, item)| item.quantity).sum()
    })
    }

// Function that modify the details of a shoe
#[ic_cdk::update]
fn update_shoe(id: u64, payload: ShoePayload) -> Result<Shoe, Error> {
    
    // Check if the caller is the owner of the shoe; if not, return an authorization error
        if !_validate_owner(&_get_shoe(&id).unwrap().clone()){
            return Err(Error::NotAuthorized {
                msg: format!(
                    "You're not the owner of the shoe with id={}",
                    id
                ),
                caller: caller()
            })
        }
    match SHOE_STORAGE.with(|service| service.borrow().get(&id)) {
        Some(mut shoe) => {
            shoe.name = payload.name;
            shoe.size = payload.size;
            shoe.price = payload.price;
            shoe.shoe_url = payload.shoe_url;
            shoe.quantity = payload.quantity;
            shoe.updated_at = Some(time());
            do_insert(&shoe);
            Ok(shoe)
        }
        None => Err(Error::NotFound {
            msg: format!(
                "couldn't update a shoe with id={}. shoe not found",
                id
            ),
        }),
    }
}

 
// Search Shoe product by Name
#[ic_cdk::query]
fn search_by_name(name: String) -> Vec<Shoe>  {
SHOE_STORAGE.with(|service| {
    let storage = service.borrow_mut();
    storage
        .iter()
        .filter(|(_, item)| item.name == name)
        .map(|(_, item)| item.clone())
        .collect()
    })
}

// Function that likes a shoe using the shoe id
#[ic_cdk::update]
fn like_shoe(id: u64) -> Result<Shoe, Error> {
    match _get_shoe(&id) {
    Some(mut likes_shoe) => { 
        let caller = caller();
        // Search for the index of the caller in the liked array
        let index = likes_shoe.liked_by.iter().position(|&user| user.to_string() == caller.to_string());
        // if an index is returned, return an error as users can only like once
        if index.is_some(){
            return Err(Error::AlreadyLiked {
                msg: format!("Shoe with ID {} has already been liked by caller: {}.", id, caller),
            });
        }
        likes_shoe.like = 1;
        likes_shoe.liked_by.push(caller);
        do_insert(&likes_shoe);
        Ok(likes_shoe.clone())
    }
    None => Err(Error::NotFound {
        msg: format!("Shoe with ID {} not found. Cannot like.", id),
    }),
}


// Update function to delete a specific shoe by its id
#[ic_cdk::update]
fn delete_shoe(id: u64) -> Result<Shoe, Error> {
// Check if the caller is the owner of the shoe; if not, return an authorization error
if !_validate_owner(&_get_shoe(&id).unwrap().clone()){
    return Err(Error::NotAuthorized {
        msg: format!(
            "You're not the store owner with id={}",
            id
        ),
        caller: caller()
    })
}
// Attempt to remove the shoe from storage based on its unique identifier
    match SHOE_STORAGE.with(|service| service.borrow_mut().remove(&id)) {
        Some(shoe) => Ok(shoe),
        None => Err(Error::NotFound {
            msg: format!(
                "couldn't delete a shoe with id={}. shoe not found.",
                id
            ),
        }),
    }
}

#[ic_cdk::query]
fn search_shoes(criteria: ShoeSearchCriteria) -> Vec<Shoe> {
    SHOE_STORAGE.with(|service| {
        service.borrow()
               .iter()
               .filter(|(_, shoe)| {
                   criteria.name.as_ref().map_or(true, |name| shoe.name == *name) &&
                   criteria.size.as_ref().map_or(true, |size| shoe.size == *size) &&
                   criteria.min_price.map_or(true, |min_price| shoe.price >= min_price) &&
                   criteria.max_price.map_or(true, |max_price| shoe.price <= max_price)
               })
               .map(|(_, shoe)| shoe.clone())
               .collect()
    })
}



#[derive(candid::CandidType, Deserialize, Serialize)]
enum Error {
    NotFound { msg: String },
    NotAuthorized {msg: String , caller: Principal},
    AlreadyLiked {msg: String},
}

// helper method to perform insert.
fn do_insert(shoe: &Shoe) {
SHOE_STORAGE.with(|service| service.borrow_mut().insert(shoe.id, shoe.clone()));
}

// a helper method to get a shoe by id. used in get_shoe/update_shoe
fn _get_shoe(id: &u64) -> Option<Shoe> {
SHOE_STORAGE.with(|service| service.borrow().get(id))
}
// Helper function to validate owner 
fn _validate_owner(shoe: &Shoe) -> bool {
    if shoe.owner.to_string() != caller().to_string(){
        return false  
    }
    return  true 

        
}

// need this to generate candid
ic_cdk::export_candid!();